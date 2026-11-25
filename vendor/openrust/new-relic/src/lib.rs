//! 通过 OTLP 将应用遥测发送至 New Relic。
#![forbid(unsafe_code)]

mod export;
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

/// 注册应用级 New Relic 遥测，默认关闭。
pub struct NewRelicServiceProvider;

impl ServiceProvider for NewRelicServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<Environment>().await?;
                let enabled = match env.get_utf8("NEW_RELIC_ENABLED")? {
                    None | Some("false" | "0") => false,
                    Some("true" | "1") => true,
                    Some(_) => return Err(invalid("NEW_RELIC_ENABLED", "true 或 false").into()),
                };
                self.merge_config_from(
                    app,
                    json!({
                        "enabled": enabled,
                        "app_name": env.get_utf8("NEW_RELIC_APP_NAME")?,
                        "region": env.get_utf8("NEW_RELIC_REGION")?.unwrap_or("us"),
                        "collector_endpoint": env.get_utf8("NEW_RELIC_COLLECTOR_ENDPOINT")?,
                        "propagate_origins": env.get_utf8("NEW_RELIC_PROPAGATE_ORIGINS")?,
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
            let recorder = tokio::task::spawn_blocking(move || {
                recorder::Recorder::new(name, destination, propagation)
            })
            .await??;
            let dispatcher = app.container().make::<openrust::events::Dispatcher>()?;
            let recorder = Arc::new(recorder);
            openrust::foundation::observability::register(app.container(), recorder.clone())?;
            recorder::register(&dispatcher, recorder)?;
            Ok(())
        })
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
