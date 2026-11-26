//! 通过 OTLP 将应用遥测发送至 New Relic。
#![forbid(unsafe_code)]

mod admission;
mod export;
pub use export::ExportStats;
mod log_handler;
pub use log_handler::NewRelicHandler;
mod recorder;

use std::sync::Arc;

use openrust::{
    config::{ConfigError, Environment},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use serde_json::json;

/// 从应用容器解析的遥测控制句柄。仅在扩展启用时绑定。
pub struct NewRelic(Arc<recorder::Recorder>);

/// 遥测刷新失败或超出等待时间。错误不包含出口地址或授权信息。
#[derive(Debug)]
pub struct FlushError;

impl std::fmt::Display for FlushError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("New Relic 遥测未能完整刷新")
    }
}

impl std::error::Error for FlushError {}

impl NewRelic {
    /// 重命名当前应用的活动事务 Span。HTTP 指标仍以匹配路由为准。
    pub fn name_transaction(&self, name: &str) -> bool {
        self.0.name_transaction(name)
    }

    /// 为当前事务添加一个应用显式选择的自定义属性。
    pub fn add_attribute(&self, key: &str, value: &str) -> bool {
        self.0.add_attribute(key, value)
    }

    /// 当前应用的出口请求尝试和失败计数。
    pub fn export_stats(&self) -> ExportStats {
        self.0.export_stats()
    }

    /// 刷新当前缓冲区，保持应用继续采集。
    pub async fn flush(&self) -> Result<(), FlushError> {
        self.0
            .export_pending(false)
            .await
            .then_some(())
            .ok_or(FlushError)
    }

    /// 停止应用采集并刷新出口。用于自行驱动应用生命周期的消费者。
    pub async fn shutdown(&self) -> Result<(), FlushError> {
        self.0
            .export_pending(true)
            .await
            .then_some(())
            .ok_or(FlushError)
    }
}

/// 注册应用级 New Relic 遥测，默认关闭。
pub struct NewRelicServiceProvider;

impl ServiceProvider for NewRelicServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<Environment>().await?;
                self.merge_config_from(
                    app,
                    json!({
                        "enabled": env_flag(&env, "NEW_RELIC_ENABLED", false)?,
                        "traces_enabled": env_flag(&env, "NEW_RELIC_TRACES_ENABLED", true)?,
                        "metrics_enabled": env_flag(&env, "NEW_RELIC_METRICS_ENABLED", true)?,
                        "instance_id": env.get_utf8("NEW_RELIC_INSTANCE_ID")?,
                        "app_name": env.get_utf8("NEW_RELIC_APP_NAME")?,
                        "region": env.get_utf8("NEW_RELIC_REGION")?.unwrap_or("us"),
                        "collector_endpoint": env.get_utf8("NEW_RELIC_COLLECTOR_ENDPOINT")?,
                        "propagate_origins": env.get_utf8("NEW_RELIC_PROPAGATE_ORIGINS")?,
                        "exception_stacktrace": env_flag(&env, "NEW_RELIC_EXCEPTION_STACKTRACE", false)?,
                        "logs_enabled": env_flag(&env, "NEW_RELIC_LOGS_ENABLED", false)?,
                        "sample_ratio": env.get_utf8("NEW_RELIC_TRACE_SAMPLE_RATIO")?,
                    }),
                    "new_relic",
                )?;
            }
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app.config()?;
            match config.get("new_relic.enabled") {
                None | Some(serde_json::Value::Bool(false)) => return Ok(()),
                Some(serde_json::Value::Bool(true)) => {}
                _ => return Err(invalid("new_relic.enabled", "布尔值").into()),
            }
            let name = config
                .get("new_relic.app_name")
                .filter(|value| !value.is_null())
                .or_else(|| config.get("app.name"));
            let name = name
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| invalid("new_relic.app_name", "非空服务名称"))?
                .to_owned();
            let env = app.make::<Environment>().await?;
            let destination =
                export::Destination::resolve(config.as_ref(), &env).inspect_err(|error| {
                    // 引导入口只显示 Provider 失败，需要单独告知具体配置项。
                    eprintln!("New Relic 配置错误：{error}");
                })?;
            let propagation = config.get("new_relic.propagate_origins");
            let propagation = match propagation.as_ref() {
                None | Some(serde_json::Value::Null) => Vec::new(),
                Some(value) => value
                    .as_str()
                    .ok_or_else(|| {
                        invalid("new_relic.propagate_origins", "逗号分隔的 HTTP Origin")
                    })?
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        url::Url::parse(value)
                            .ok()
                            .filter(|url| {
                                matches!(url.scheme(), "http" | "https")
                                    && url.host().is_some()
                                    && url.username().is_empty()
                                    && url.password().is_none()
                                    && url.path() == "/"
                                    && url.query().is_none()
                                    && url.fragment().is_none()
                            })
                            .map(|url| url.origin())
                            .ok_or_else(|| {
                                invalid(
                                    "new_relic.propagate_origins",
                                    "不含路径或凭据的 HTTP Origin",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            };
            let setting = |key, default| match config.get(key) {
                None => Ok(default),
                Some(value) => value.as_bool().ok_or_else(|| invalid(key, "布尔值")),
            };
            let string = |key| match config.get(key).filter(|value| !value.is_null()) {
                None => Ok(None),
                Some(value) => value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| Some(value.chars().take(128).collect::<String>()))
                    .ok_or_else(|| invalid(key, "非空字符串")),
            };
            let ratio = match config
                .get("new_relic.sample_ratio")
                .filter(|value| !value.is_null())
            {
                None => 1.0,
                Some(value) => value
                    .as_f64()
                    .or_else(|| value.as_str()?.parse().ok())
                    .filter(|ratio| (0.0..=1.0).contains(ratio))
                    .ok_or_else(|| invalid("new_relic.sample_ratio", "0 到 1 之间的数值"))?,
            };
            let settings = recorder::Settings {
                name,
                version: string("app.version")?,
                instance_id: string("new_relic.instance_id")?,
                propagate_origins: propagation,
                exception_stacktrace: setting("new_relic.exception_stacktrace", false)?,
                logs_enabled: setting("new_relic.logs_enabled", false)?,
                traces_enabled: setting("new_relic.traces_enabled", true)?,
                metrics_enabled: setting("new_relic.metrics_enabled", true)?,
                ratio,
            };
            let recorder =
                tokio::task::spawn_blocking(move || recorder::Recorder::new(settings, destination))
                    .await??;
            let dispatcher = app.container().make::<openrust::events::Dispatcher>()?;
            let recorder = Arc::new(recorder);
            if let Some(logger) = recorder.logger() {
                let log_recorder = recorder.clone();
                let log_manager = app.container().make::<openrust::log::LogManager>()?;
                log_manager.register_formattable_handler("new_relic", move |arguments| {
                    Ok(Arc::new(NewRelicHandler::from_arguments(
                        log_recorder.clone(),
                        logger.clone(),
                        arguments,
                    )?))
                });
            }
            app.container()
                .instance(Arc::new(NewRelic(recorder.clone())))?;
            openrust::foundation::observability::register(app.container(), recorder.clone())?;
            recorder::register(&dispatcher, recorder)?;
            Ok(())
        })
    }
}

fn env_flag(env: &Environment, key: &str, default: bool) -> Result<bool, ConfigError> {
    match env.get_utf8(key)? {
        None => Ok(default),
        Some("false" | "0") => Ok(false),
        Some("true" | "1") => Ok(true),
        Some(_) => Err(invalid(key, "true 或 false")),
    }
}

fn invalid(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.into(),
        expected,
    }
}

/// 编译期包发现入口。
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(NewRelicServiceProvider)]
}
