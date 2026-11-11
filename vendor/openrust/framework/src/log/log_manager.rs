use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use serde::Deserialize;
use serde_json::Value;

use crate::contracts::{config::Repository, log::Logger as LoggerContract};

use super::{
    Level, LogContext, LogError, LogResult, Logger,
    logger::Emergency,
    rotating::{Period, RotatingFile},
    sink::{FileOptions, Handler, Handlers},
};

const DEFAULT_FILE: &str = "storage/logs/openrust.log";
const MAX_STACK_DEPTH: usize = 32;
type Factory = dyn Fn(&LogManager, &Value) -> LogResult<Arc<dyn LoggerContract>> + Send + Sync;
type Factories = Mutex<HashMap<String, Arc<Factory>>>;

thread_local! {
    static ACTIVE_FACTORIES: RefCell<Vec<(usize, usize, String)>> = const { RefCell::new(Vec::new()) };
}

struct FactoryCall;

impl FactoryCall {
    fn enter(manager: &LogManager, name: &str, factory: &Arc<Factory>) -> LogResult<Self> {
        let key = (
            std::ptr::from_ref(manager).addr(),
            Arc::as_ptr(factory).addr(),
            name.to_owned(),
        );
        ACTIVE_FACTORIES.with_borrow_mut(|active| {
            if active.len() >= MAX_STACK_DEPTH || active.contains(&key) {
                return Err(LogError::CyclicStack);
            }
            active.push(key);
            Ok(Self)
        })
    }
}

impl Drop for FactoryCall {
    fn drop(&mut self) {
        ACTIVE_FACTORIES.with_borrow_mut(|active| {
            active.pop();
        });
    }
}

/// A named channel or a resolved on-demand logger in a temporary stack.
#[derive(Clone, Copy)]
pub enum StackChannel<'a> {
    Named(&'a str),
    Logger(&'a Logger),
}

/// Resolves the application's named log channels and stacks.
pub struct LogManager {
    config: Arc<dyn Repository>,
    root: PathBuf,
    context_owner: Weak<crate::container::Container>,
    channels: Mutex<ChannelDirectory>,
    drivers: Factories,
    factories: Factories,
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
            context_owner: Weak::new(),
            channels: Mutex::default(),
            drivers: Mutex::default(),
            factories: Mutex::default(),
        }
    }

    pub(super) fn with_context_owner(mut self, owner: Weak<crate::container::Container>) -> Self {
        self.context_owner = owner;
        self
    }

    /// Registers a named driver. Existing cached channels retain their current driver.
    pub fn extend(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(&Self, &Value) -> LogResult<Arc<dyn LoggerContract>> + Send + Sync + 'static,
    ) {
        register(&self.drivers, driver.into(), Arc::new(factory));
    }

    /// Registers a compiled factory referenced by a custom channel's `via` option.
    pub fn registerFactory(
        &self,
        via: impl Into<String>,
        factory: impl Fn(&Self, &Value) -> LogResult<Arc<dyn LoggerContract>> + Send + Sync + 'static,
    ) {
        register(&self.factories, via.into(), Arc::new(factory));
    }

    /// Resolves an on-demand channel from explicit settings without caching it by name.
    pub fn build(&self, config: &Value) -> LogResult<Arc<Logger>> {
        self.resolve(|resolution| {
            resolution.ancestors.push("ondemand".to_owned());
            let logger = resolution.build("ondemand", config);
            Ok(resolution.or_emergency(logger))
        })
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
            Ok(resolution.or_emergency(channel))
        })
    }

    /// Creates a stack without inserting it into the named-channel cache.
    pub fn stack(&self, names: &[&str], name: Option<&str>) -> LogResult<Arc<Logger>> {
        self.stack_items(names.iter().copied().map(StackChannel::Named), name)
    }

    /// Creates a stack of named channels and existing loggers.
    pub fn stackChannels(
        &self,
        channels: &[StackChannel<'_>],
        name: Option<&str>,
    ) -> LogResult<Arc<Logger>> {
        self.stack_items(channels.iter().copied(), name)
    }

    fn stack_items<'a>(
        &self,
        channels: impl Iterator<Item = StackChannel<'a>>,
        name: Option<&str>,
    ) -> LogResult<Arc<Logger>> {
        self.resolve(|resolution| {
            let (handlers, replace_placeholders) = resolution.stack(channels, false)?;
            Ok(Arc::new(Logger {
                name: name.unwrap_or(&resolution.settings.environment).to_owned(),
                handlers: Some(handlers),
                replace_placeholders,
                emergency: resolution.settings.emergency.clone(),
                context_owner: resolution.settings.context_owner.clone(),
            }))
        })
    }

    fn resolve(
        &self,
        build: impl FnOnce(&mut Resolution<'_, '_>) -> LogResult<Arc<Logger>>,
    ) -> LogResult<Arc<Logger>> {
        let generation = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .generation;
        let settings = self.settings();
        let mut diagnostics = Vec::new();
        let result = build(&mut Resolution {
            manager: self,
            generation,
            diagnostics: &mut diagnostics,
            settings: &settings,
            ancestors: Vec::new(),
        });
        report(&settings.emergency, &diagnostics)?;
        result
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
            context_owner: &self.context_owner,
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
    #[allow(non_snake_case)]
    fn logWithContext(
        &self,
        level: Level,
        message: &str,
        context: &LogContext,
        extra: &LogContext,
    ) -> LogResult<()> {
        self.channel(None)?
            .logWithContext(level, message, context, extra)
    }
}

struct Settings<'a> {
    config: Value,
    context_owner: &'a Weak<crate::container::Container>,
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
            context_owner: self.context_owner.clone(),
        })
    }
}

struct Resolution<'a, 'settings> {
    manager: &'a LogManager,
    generation: u64,
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
        {
            let channels = self
                .manager
                .channels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if channels.generation == self.generation
                && let Some(logger) = channels.entries.get(name)
            {
                return Ok(logger.clone());
            }
        }
        let config = self
            .settings
            .config
            .get(name)
            .ok_or_else(|| LogError::UnknownChannel(name.to_owned()))?;
        self.ancestors.push(name.to_owned());
        let built = self.build(name, config);
        self.ancestors.pop();
        let logger = built?;
        let mut channels = self
            .manager
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if channels.generation != self.generation {
            return Ok(logger);
        }
        Ok(channels
            .entries
            .entry(name.to_owned())
            .or_insert_with(|| logger.clone())
            .clone())
    }

    fn or_emergency(&mut self, logger: LogResult<Arc<Logger>>) -> Arc<Logger> {
        match logger {
            Ok(logger) => logger,
            Err(error) => {
                self.diagnostics.push(error);
                self.settings.fallback()
            }
        }
    }

    fn build(&mut self, name: &str, config: &Value) -> LogResult<Arc<Logger>> {
        let options: Options =
            Options::deserialize(config).map_err(|_| LogError::InvalidConfiguration)?;
        let (handlers, replace_placeholders) = self.handlers(name, &options, config)?;
        Ok(Arc::new(Logger {
            name: options
                .name
                .unwrap_or_else(|| self.settings.environment.clone()),
            handlers: Some(handlers),
            replace_placeholders,
            emergency: self.settings.emergency.clone(),
            context_owner: self.settings.context_owner.clone(),
        }))
    }

    fn handlers(
        &mut self,
        name: &str,
        options: &Options,
        config: &Value,
    ) -> LogResult<(Handlers, bool)> {
        let level = options
            .level
            .as_deref()
            .map(Level::parse)
            .transpose()?
            .unwrap_or(Level::Debug);
        let factory = find(&self.manager.drivers, &options.driver).or_else(|| {
            if options.driver == "custom" {
                options
                    .via
                    .as_deref()
                    .and_then(|via| find(&self.manager.factories, via))
            } else {
                None
            }
        });
        if let Some(factory) = factory {
            let _call = FactoryCall::enter(self.manager, name, &factory)?;
            let logger = factory(self.manager, config).map_err(|_| LogError::FactoryFailed)?;
            let handler = Handler::Custom {
                logger,
                level,
                bubble: options.bubble,
            };
            return Ok(([Arc::new(handler)].into(), options.replace_placeholders));
        }
        if options.formatter.is_some() {
            return Err(LogError::InvalidConfiguration);
        }
        let driver: Driver =
            Driver::deserialize(config).map_err(|_| LogError::InvalidConfiguration)?;
        if let Driver::Stack {
            channels,
            ignore_exceptions,
        } = &driver
        {
            return self.stack(
                channels.names().map(StackChannel::Named),
                *ignore_exceptions,
            );
        }
        let handler = match driver {
            Driver::Single(file) => {
                let options_file = file.options(self.settings.root)?;
                Handler::file(
                    options_file.path,
                    options_file.permission,
                    options_file.locking,
                    level,
                    options.bubble,
                )
            }
            Driver::Daily {
                file,
                max_files,
                days,
            } => self.rotating(
                file,
                Period::Daily,
                max_files.or(days).unwrap_or(7),
                level,
                options.bubble,
            )?,
            Driver::Monthly { file, max_files } => self.rotating(
                file,
                Period::Monthly,
                max_files.unwrap_or(3),
                level,
                options.bubble,
            )?,
            Driver::Stderr => Handler::Stderr {
                level,
                bubble: options.bubble,
            },
            Driver::Null => Handler::Null(level),
            Driver::Stack { .. } => unreachable!("stack handled before leaf drivers"),
        };
        Ok(([Arc::new(handler)].into(), options.replace_placeholders))
    }

    fn rotating(
        &self,
        file: FileConfig,
        period: Period,
        max_files: usize,
        level: Level,
        bubble: bool,
    ) -> LogResult<Handler> {
        let file = RotatingFile::new(file.options(self.settings.root)?, period, max_files)
            .ok_or(LogError::InvalidConfiguration)?;
        Ok(Handler::Rotating {
            file,
            level,
            bubble,
        })
    }

    fn stack<'a>(
        &mut self,
        channels: impl Iterator<Item = StackChannel<'a>>,
        ignore: bool,
    ) -> LogResult<(Handlers, bool)> {
        let mut handlers = Vec::new();
        let mut replace_placeholders = false;
        for member in channels {
            let resolved;
            let logger = match member {
                StackChannel::Logger(logger) => logger,
                StackChannel::Named(name) => {
                    let channel = self.channel(name.trim());
                    if matches!(channel, Err(LogError::CyclicStack)) {
                        return Err(LogError::CyclicStack);
                    }
                    resolved = self.or_emergency(channel);
                    &resolved
                }
            };
            handlers.extend(
                logger
                    .handlers
                    .as_deref()
                    .unwrap_or(&logger.emergency.handlers)
                    .iter()
                    .cloned(),
            );
            replace_placeholders |= logger.replace_placeholders;
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
    driver: String,
    via: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "driver", rename_all = "lowercase")]
enum Driver {
    Single(FileConfig),
    Daily {
        #[serde(flatten)]
        file: FileConfig,
        max_files: Option<usize>,
        days: Option<usize>,
    },
    Monthly {
        #[serde(flatten)]
        file: FileConfig,
        max_files: Option<usize>,
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
struct FileConfig {
    path: String,
    permission: Option<u32>,
    #[serde(default)]
    locking: bool,
}

impl FileConfig {
    fn options(self, root: &std::path::Path) -> LogResult<FileOptions> {
        if self.path.is_empty() || self.permission.is_some_and(|permission| permission > 0o777) {
            return Err(LogError::InvalidConfiguration);
        }
        Ok(FileOptions {
            path: root.join(self.path),
            permission: self.permission,
            locking: self.locking,
        })
    }
}

fn register(directory: &Factories, key: String, factory: Arc<Factory>) {
    let retired = directory
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, factory);
    drop(retired);
}

fn find(directory: &Factories, key: &str) -> Option<Arc<Factory>> {
    directory
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(key)
        .cloned()
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
        "daily": { "driver": "daily", "path": DEFAULT_FILE, "max_files": 14, "replace_placeholders": true },
        "monthly": { "driver": "monthly", "path": DEFAULT_FILE, "max_files": 3, "replace_placeholders": true },
        "stderr": { "driver": "stderr", "replace_placeholders": true },
        "null": { "driver": "null" }
    })
}
