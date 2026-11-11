use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::Deserialize;
use serde_json::Value;

use crate::contracts::config::Repository;

use super::{
    Level, LogContext, LogError, LogResult, Logger,
    logger::Emergency,
    sink::{Handler, Handlers},
};

const DEFAULT_FILE: &str = "storage/logs/openrust.log";
const MAX_STACK_DEPTH: usize = 32;

/// Resolves the application's named log channels and stacks.
pub struct LogManager {
    config: Arc<dyn Repository>,
    root: PathBuf,
    channels: Mutex<ChannelDirectory>,
}

#[derive(Default)]
struct ChannelDirectory {
    generation: u64,
    entries: HashMap<String, Arc<Logger>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's LogManager API")]
impl LogManager {
    pub fn new(config: Arc<dyn Repository>, root: impl Into<PathBuf>) -> Self {
        Self {
            config,
            root: root.into(),
            channels: Mutex::default(),
        }
    }

    pub fn getDefaultDriver(&self) -> Option<String> {
        match self.config.get("logging.default") {
            Some(Value::String(name)) => Some(name),
            None if !self.config.has("logging") => Some("stack".to_owned()),
            _ => None,
        }
    }

    pub fn setDefaultDriver(&self, name: &str) -> Result<(), crate::config::ConfigError> {
        self.config
            .set("logging.default", Value::String(name.to_owned()))
    }

    pub fn forgetChannel(&self, name: Option<&str>) {
        let Some(name) = self.channel_name(name) else {
            return;
        };
        let mut channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        channels.generation = channels.generation.wrapping_add(1);
        channels.entries.remove(name.trim());
    }

    fn channel_name(&self, name: Option<&str>) -> Option<String> {
        name.map(str::to_owned)
            .or_else(|| self.getDefaultDriver())
            .or_else(|| {
                (self.config.get("app.env").as_ref().and_then(Value::as_str) == Some("testing"))
                    .then(|| "null".to_owned())
            })
    }

    pub fn driver(&self, name: Option<&str>) -> LogResult<Arc<Logger>> {
        self.channel(name)
    }

    pub fn channel(&self, name: Option<&str>) -> LogResult<Arc<Logger>> {
        let name = self.channel_name(name);
        let name = name.as_deref().map(str::trim);
        if let Some(logger) = name.and_then(|name| {
            self.channels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entries
                .get(name)
                .cloned()
        }) {
            return Ok(logger);
        }
        self.resolve(|resolution| {
            let channel = name
                .ok_or(LogError::InvalidConfiguration)
                .and_then(|name| resolution.channel(name));
            Ok(match channel {
                Ok(logger) => logger,
                Err(error) => {
                    resolution.diagnostics.push(error);
                    resolution.settings.fallback()
                }
            })
        })
    }

    /// Creates a stack without inserting it into the named-channel cache.
    pub fn stack(&self, names: &[&str], name: Option<&str>) -> LogResult<Arc<Logger>> {
        self.resolve(|resolution| {
            let (handlers, replace_placeholders) =
                resolution.stack(names.iter().copied(), false)?;
            Ok(Arc::new(Logger {
                name: name.unwrap_or(&resolution.settings.environment).to_owned(),
                handlers: Some(handlers),
                replace_placeholders,
                emergency: resolution.settings.emergency.clone(),
            }))
        })
    }

    fn resolve(
        &self,
        build: impl FnOnce(&mut Resolution<'_, '_>) -> LogResult<Arc<Logger>>,
    ) -> LogResult<Arc<Logger>> {
        let lock = || {
            self.channels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        };
        loop {
            let generation = lock().generation;
            let settings = self.settings();
            let mut channels = lock();
            if generation != channels.generation {
                continue;
            }
            let mut diagnostics = Vec::new();
            let result = build(&mut Resolution {
                channels: &mut channels.entries,
                diagnostics: &mut diagnostics,
                settings: &settings,
                ancestors: Vec::new(),
            });
            drop(channels);
            report(&settings.emergency, &diagnostics)?;
            return result;
        }
    }

    pub fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        self.channel(None)?.log(level, message, context)
    }

    log_methods!(self);

    fn settings(&self) -> Settings<'_> {
        let config = self
            .config
            .get("logging.channels")
            .unwrap_or_else(default_channels);
        let path = config["emergency"]["path"]
            .as_str()
            .filter(|path| !path.is_empty())
            .unwrap_or(DEFAULT_FILE);
        let emergency = Arc::new(Emergency {
            handlers: [Arc::new(Handler::file(
                self.root.join(path),
                None,
                false,
                Level::Debug,
                true,
            ))]
            .into(),
        });
        Settings {
            config,
            root: &self.root,
            environment: self
                .config
                .get("app.env")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "production".to_owned()),
            emergency,
        }
    }
}

impl crate::contracts::log::Logger for LogManager {
    fn log(&self, level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::log(self, level, message, context)
    }
}

struct Settings<'a> {
    config: Value,
    root: &'a std::path::Path,
    environment: String,
    emergency: Arc<Emergency>,
}

impl Settings<'_> {
    fn fallback(&self) -> Arc<Logger> {
        Arc::new(Logger {
            name: "openrust".to_owned(),
            handlers: None,
            replace_placeholders: false,
            emergency: self.emergency.clone(),
        })
    }
}

struct Resolution<'a, 'settings> {
    channels: &'a mut HashMap<String, Arc<Logger>>,
    diagnostics: &'a mut Vec<LogError>,
    settings: &'a Settings<'settings>,
    ancestors: Vec<String>,
}

impl Resolution<'_, '_> {
    fn channel(&mut self, name: &str) -> LogResult<Arc<Logger>> {
        if self.ancestors.len() >= MAX_STACK_DEPTH
            || self.ancestors.iter().any(|parent| parent == name)
        {
            return Err(LogError::CyclicStack);
        }
        if let Some(logger) = self.channels.get(name) {
            return Ok(logger.clone());
        }
        let settings = self.settings;
        let config = settings
            .config
            .get(name)
            .ok_or_else(|| LogError::UnknownChannel(name.to_owned()))?;
        let options: Options =
            serde_json::from_value(config.clone()).map_err(|_| LogError::InvalidConfiguration)?;
        self.ancestors.push(name.to_owned());
        let handlers = self.handlers(&options);
        self.ancestors.pop();
        let (handlers, replace_placeholders) = handlers?;
        let logger = Arc::new(Logger {
            name: options.name.unwrap_or_else(|| settings.environment.clone()),
            handlers: Some(handlers),
            replace_placeholders,
            emergency: settings.emergency.clone(),
        });
        self.channels.insert(name.to_owned(), logger.clone());
        Ok(logger)
    }

    fn handlers(&mut self, options: &Options) -> LogResult<(Handlers, bool)> {
        if options.formatter.is_some() {
            return Err(LogError::InvalidConfiguration);
        }
        if let Driver::Stack {
            channels,
            ignore_exceptions,
        } = &options.driver
        {
            return self.stack(channels.names(), *ignore_exceptions);
        }
        let level = options
            .level
            .as_deref()
            .map(Level::parse)
            .transpose()?
            .unwrap_or(Level::Debug);
        let handler = match &options.driver {
            Driver::Single {
                path,
                permission,
                locking,
            } => {
                if path.is_empty() || permission.is_some_and(|permission| permission > 0o777) {
                    return Err(LogError::InvalidConfiguration);
                }
                Handler::file(
                    self.settings.root.join(path),
                    *permission,
                    *locking,
                    level,
                    options.bubble,
                )
            }
            Driver::Stderr => Handler::Stderr {
                level,
                bubble: options.bubble,
            },
            Driver::Null => Handler::Null(level),
            Driver::Stack { .. } => unreachable!("stack handled before leaf drivers"),
        };
        Ok(([Arc::new(handler)].into(), options.replace_placeholders))
    }

    fn stack<'a>(
        &mut self,
        names: impl Iterator<Item = &'a str>,
        ignore: bool,
    ) -> LogResult<(Handlers, bool)> {
        let mut handlers = Vec::new();
        let mut replace_placeholders = false;
        for name in names {
            match self.channel(name.trim()) {
                Ok(logger) => {
                    handlers.extend(
                        logger
                            .handlers
                            .as_deref()
                            .expect("cached channels have handlers")
                            .iter()
                            .cloned(),
                    );
                    replace_placeholders |= logger.replace_placeholders;
                }
                Err(LogError::CyclicStack) => return Err(LogError::CyclicStack),
                Err(error) => {
                    self.diagnostics.push(error);
                    handlers.extend(self.settings.emergency.handlers.iter().cloned());
                }
            }
        }
        let handlers = if ignore {
            [Arc::new(Handler::IgnoreFailures(handlers.into()))].into()
        } else {
            handlers.into()
        };
        Ok((handlers, replace_placeholders))
    }
}

fn report(emergency: &Emergency, diagnostics: &[LogError]) -> LogResult<()> {
    for cause in diagnostics {
        emergency.write(
            Level::Emergency,
            "Unable to create configured logger. Using emergency logger.",
            &LogContext::from_iter([("reason".into(), Value::String(cause.to_string()))]),
        )?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct Options {
    name: Option<String>,
    formatter: Option<String>,
    level: Option<String>,
    #[serde(default = "default_bubble")]
    bubble: bool,
    #[serde(default)]
    replace_placeholders: bool,
    #[serde(flatten)]
    driver: Driver,
}

#[derive(Deserialize)]
#[serde(tag = "driver", rename_all = "lowercase")]
enum Driver {
    Single {
        path: String,
        permission: Option<u32>,
        #[serde(default)]
        locking: bool,
    },
    Stderr,
    Null,
    Stack {
        channels: Channels,
        #[serde(default)]
        ignore_exceptions: bool,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Channels {
    Text(String),
    List(Vec<String>),
}

impl Channels {
    fn names(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Text(text) => Box::new(text.split(',')),
            Self::List(names) => Box::new(names.iter().map(String::as_str)),
        }
    }
}

const fn default_bubble() -> bool {
    true
}

fn default_channels() -> Value {
    serde_json::json!({
        "stack": { "driver": "stack", "channels": ["single"] },
        "single": { "driver": "single", "path": DEFAULT_FILE, "replace_placeholders": true },
        "stderr": { "driver": "stderr", "replace_placeholders": true },
        "null": { "driver": "null" }
    })
}
