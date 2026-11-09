//! 应用日志频道、惰性解析与驱动配置。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::OnceCell;

use super::{LogLevel, LogSink, Logger, sinks};
use crate::{
    Error, Result,
    config::{ConfigContext, ConfigError, repository::ConfigRepository},
    foundation::RegisterContext,
    support::{BoxFuture, ServiceProvider},
};

const DEFAULT_FILE: &str = "storage/logs/openrust.log";
const MAX_STACK_DEPTH: usize = 32;
type Factory = dyn Fn(&Value) -> Result<Arc<dyn LogSink>> + Send + Sync;
type ResolvedChannel = Arc<OnceCell<Arc<Logger>>>;

/// 应用内的默认和具名日志频道管理器。
///
/// 频道惰性解析。已解析的频道共享后端；重新登记 driver 后调用
/// `forget_channel` 即可按当前配置重建。
pub struct LogManager {
    config: ConfigRepository,
    scope_factory: crate::container::ScopeFactory,
    drivers: Mutex<HashMap<String, Arc<Factory>>>,
    channels: Mutex<ChannelDirectory>,
}

#[derive(Default)]
struct ChannelDirectory {
    generation: u64,
    entries: HashMap<String, ResolvedChannel>,
}

impl LogManager {
    fn new(config: ConfigRepository, scope_factory: crate::container::ScopeFactory) -> Self {
        Self {
            config,
            scope_factory,
            drivers: Mutex::default(),
            channels: Mutex::default(),
        }
    }

    /// 登记或替换具名 driver。构造器接收整个频道配置，不在目录锁内执行。
    /// 构造器应快速返回；写入与网络 I/O 由 sink 执行。
    pub fn extend(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(&Value) -> Result<Arc<dyn LogSink>> + Send + Sync + 'static,
    ) {
        let name = driver.into();
        let retired = lock(&self.drivers).insert(name, Arc::new(factory));
        drop(retired);
    }

    /// 取得默认或具名频道。频道初始化失败时，写入应急记录并返回应急 logger。
    ///
    /// 应急记录自身写入失败时返回错误。失败的频道不会被缓存。
    pub async fn channel(&self, name: Option<&str>) -> Result<Arc<Logger>> {
        let generation = lock(&self.channels).generation;
        let settings = self.selection()?;
        let result = match settings.name(name) {
            Some(name) => self.build(&settings, name, Vec::new(), generation).await,
            None => Err(undefined()),
        };
        match result {
            Ok(channel) => Ok(channel),
            Err(cause) => emergency(&settings, &cause).await,
        }
    }

    /// 移除已解析的默认或具名频道；已经注入的 logger 仍持有原后端。
    pub fn forget_channel(&self, name: Option<&str>) -> Result<()> {
        if let Some(name) = self.selection()?.name(name) {
            let retired = {
                let mut channels = lock(&self.channels);
                channels.generation = channels.generation.wrapping_add(1);
                channels.entries.remove(&name)
            };
            drop(retired);
        }
        Ok(())
    }

    fn selection(&self) -> Result<Settings> {
        let mut snapshot = self.config.all()?;
        let environment = snapshot["app"]["env"]
            .as_str()
            .unwrap_or("production")
            .to_owned();
        let root = snapshot
            .as_object_mut()
            .and_then(|config| config.remove("application_root"))
            .unwrap_or(Value::Null);
        let root = serde_json::from_value(root)
            .map_err(|_| ConfigError::diagnostic("application_root", "invalid_path"))?;
        let config = snapshot
            .as_object_mut()
            .and_then(|config| config.remove("logging"))
            .unwrap_or_else(defaults);
        Ok(Settings {
            config,
            environment,
            root,
        })
    }

    fn build<'a>(
        &'a self,
        settings: &'a Settings,
        name: String,
        mut ancestors: Vec<String>,
        generation: u64,
    ) -> BoxFuture<'a, Result<Arc<Logger>>> {
        Box::pin(async move {
            if ancestors.len() == MAX_STACK_DEPTH || ancestors.contains(&name) {
                return Err(Error::custom("log_channel_cycle", "日志频道栈循环或过深"));
            }
            if let Some(channel) = lock(&self.channels)
                .entries
                .get(&name)
                .and_then(|cell| cell.get())
                .cloned()
            {
                return Ok(channel);
            }

            let config = settings.config["channels"]
                .get(&name)
                .ok_or_else(undefined)?;
            let driver = config["driver"].as_str().ok_or_else(invalid)?;
            let custom = lock(&self.drivers).get(driver).cloned();
            let label = match config.get("name") {
                None | Some(Value::Null) => &settings.environment,
                Some(name) => name.as_str().ok_or_else(invalid)?,
            };
            // 在展开子频道前保留本轮缓存槽。forget 摘除后，旧构造不会重新发布。
            let cell = {
                let mut channels = lock(&self.channels);
                if channels.generation == generation {
                    channels.entries.entry(name.clone()).or_default().clone()
                } else {
                    // 已被 forget 跨越的快照可完成原调用，不能污染当前目录。
                    Arc::default()
                }
            };
            let stack = if driver == "stack" && custom.is_none() {
                let options: StackOptions =
                    serde_json::from_value(config.clone()).map_err(|_| invalid())?;
                ancestors.push(name.clone());
                let mut sinks = Vec::new();
                for child in options.channels.names() {
                    let child = trim_name(&child).to_owned();
                    let channel = match self
                        .build(settings, child, ancestors.clone(), generation)
                        .await
                    {
                        Ok(channel) => channel,
                        Err(error) if error.code() == "log_channel_cycle" => return Err(error),
                        Err(error) => emergency(settings, &error).await?,
                    };
                    match &channel.handlers {
                        Some(handlers) => sinks.extend(handlers.iter().cloned()),
                        None => sinks.push(Arc::clone(&channel.sink)),
                    }
                }
                let ignore = options.ignore_exceptions.unwrap_or_default();
                let handlers: Arc<[_]> = sinks.into();
                let sink = sinks::stack(Arc::clone(&handlers), ignore);
                Some((sink, (!ignore).then_some(handlers)))
            } else {
                None
            };

            // 先解析栈子频道，再进入本频道的 OnceCell，避免跨频道相互等待。
            let channel = cell
                .get_or_try_init(|| async {
                    let (sink, handlers) = match (custom, stack) {
                        (Some(factory), _) => (
                            factory(config).map_err(|_| {
                                Error::custom(
                                    "log_custom_driver_failed",
                                    "自定义日志 driver 创建失败",
                                )
                            })?,
                            None,
                        ),
                        (_, Some(stack)) => stack,
                        _ => (builtin(driver, config, settings)?, None),
                    };
                    Ok::<_, Error>(Arc::new(Logger {
                        name: Arc::from(label),
                        sink,
                        handlers,
                        scope_factory: Some(self.scope_factory.clone()),
                    }))
                })
                .await?;
            Ok(Arc::clone(channel))
        })
    }
}

struct Settings {
    config: Value,
    environment: String,
    root: Option<PathBuf>,
}

impl Settings {
    fn name(&self, name: Option<&str>) -> Option<String> {
        name.or_else(|| self.config["default"].as_str())
            .or_else(|| (self.environment == "testing").then_some("null"))
            .map(|name| trim_name(name).to_owned())
    }

    fn path(&self, path: &Path) -> PathBuf {
        self.root
            .as_ref()
            .map_or_else(|| path.to_owned(), |root| root.join(path))
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Deserialize)]
struct StackOptions {
    channels: Channels,
    ignore_exceptions: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Channels {
    List(Vec<String>),
    Text(String),
}

impl Channels {
    fn names(self) -> Vec<String> {
        match self {
            Self::List(names) => names,
            Self::Text(text) => text.split(',').map(str::to_owned).collect(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SinkOptions {
    level: Option<LogLevel>,
    replace_placeholders: Option<bool>,
    path: Option<PathBuf>,
}

fn builtin(driver: &str, config: &Value, settings: &Settings) -> Result<Arc<dyn LogSink>> {
    if driver == "null" {
        return Ok(sinks::null());
    }
    let options: SinkOptions = serde_json::from_value(config.clone()).map_err(|_| invalid())?;
    let level = options.level.unwrap_or_default();
    let replace_placeholders = options.replace_placeholders.unwrap_or_default();
    match driver {
        "single" => {
            let path = options
                .path
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or_else(invalid)?;
            Ok(sinks::single(
                settings.path(&path),
                level,
                replace_placeholders,
            ))
        }
        "stderr" => Ok(sinks::stderr(level, replace_placeholders)),
        _ => Err(Error::custom(
            "log_driver_unsupported",
            "日志频道的 driver 未登记",
        )),
    }
}

async fn emergency(settings: &Settings, cause: &Error) -> Result<Arc<Logger>> {
    let path = match &settings.config["channels"]["emergency"]["path"] {
        Value::Null => DEFAULT_FILE,
        path => path
            .as_str()
            .filter(|path| !path.is_empty())
            .ok_or_else(invalid)?,
    };
    let logger = Arc::new(Logger {
        name: Arc::from("openrust"),
        sink: sinks::single(settings.path(Path::new(path)), LogLevel::Debug, false),
        handlers: None,
        scope_factory: None,
    });
    logger
        .log(
            LogLevel::Emergency,
            "Unable to create configured logger. Using emergency logger.",
            Map::from_iter([("code".into(), Value::String(cause.code().to_owned()))]),
        )
        .await?;
    Ok(logger)
}

fn trim_name(name: &str) -> &str {
    name.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{000b}'])
}

fn invalid() -> Error {
    Error::custom("log_channel_invalid", "日志频道配置无效")
}

fn undefined() -> Error {
    Error::custom("log_channel_undefined", "日志频道未配置")
}

fn defaults() -> Value {
    json!({
        "default": "stack",
        "deprecations": { "channel": "null", "trace": false, "while_testing": false },
        "channels": {
            "stack": {"driver": "stack", "channels": ["single"], "ignore_exceptions": false},
            "single": {"driver": "single", "path": DEFAULT_FILE, "level": "debug", "replace_placeholders": true},
            "stderr": {"driver": "stderr", "level": "debug", "replace_placeholders": true},
            "null": {"driver": "null"},
            "emergency": {"path": DEFAULT_FILE}
        }
    })
}

/// 构造应用和生成项目共用的日志频道配置，部署变量由当前配置上下文求值。
pub fn configuration(ctx: &ConfigContext) -> Value {
    let mut settings = defaults();
    settings["default"] = json!(ctx.nullable_string_or("LOG_CHANNEL", Some("stack")));
    settings["channels"]["stack"]["channels"] = json!(
        ctx.nullable_string_or("LOG_STACK", Some("single"))
            .unwrap_or_default()
    );
    let level = ctx.nullable_string_or("LOG_LEVEL", Some("debug"));
    settings["channels"]["single"]["level"] = json!(level);
    settings["channels"]["stderr"]["level"] = json!(level);
    settings["deprecations"] = json!({
        "channel": ctx.nullable_string_or("LOG_DEPRECATIONS_CHANNEL", Some("null")),
        "trace": ctx.nullable_bool_or("LOG_DEPRECATIONS_TRACE", Some(false)),
        "while_testing": ctx.nullable_bool_or("LOG_DEPRECATIONS_WHILE_TESTING", Some(false))
    });
    settings
}

/// 在应用的统一配置仓储和 Container 中登记日志频道管理器。
pub struct LogServiceProvider;

impl ServiceProvider for LogServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.replace_defaults_recursively("logging", defaults())?;
            app.instance(Arc::new(LogManager::new(
                app.config_repository(),
                app.scope_factory(),
            )))
            .await
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/logging/manager.rs"]
mod tests;
