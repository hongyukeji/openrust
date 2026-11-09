//! HTTP服务配置、readiness和serve命令的Provider。

use crate::core::Result;
use crate::foundation::BootContext;
use crate::http::client_ip::validate_trusted_proxy_ranges;
use crate::http::health;
use crate::http::serve::{ServeCommand, StartCommand};
#[cfg(test)]
use crate::routing::RouteRegistry;
use std::sync::Arc;

/// 声明 HTTP 服务，在 boot 阶段贡献 serve、健康探针与可选指标路由。
pub struct HttpServiceProvider;

impl crate::support::ServiceProvider for HttpServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as Schema;
            let strings = || Schema::array(Schema::String);
            let schema = Schema::object([
                ("host", Schema::String),
                ("port", Schema::Integer),
                ("public_dir", Schema::String),
                ("root_target", Schema::String),
                ("request_timeout_secs", Schema::Integer),
                ("max_concurrent_requests", Schema::Integer),
                ("shutdown_timeout_secs", Schema::Integer),
                ("metrics_endpoint_enabled", Schema::Boolean),
                ("trusted_proxy_ranges", strings().nullable()),
                ("trusted_proxy_headers", Schema::Integer),
                (
                    "trusted_hosts",
                    Schema::object([
                        ("at", strings().nullable()),
                        ("subdomains", Schema::Boolean),
                    ])
                    .nullable(),
                ),
                (
                    "compression",
                    Schema::object([("enabled", Schema::Boolean)]),
                ),
                (
                    "cors",
                    Schema::object([
                        ("enabled", Schema::Boolean),
                        ("allowed_origins", strings()),
                        ("allowed_methods", strings()),
                        ("allowed_headers", strings()),
                        ("expose_headers", strings()),
                        ("allow_credentials", Schema::Boolean),
                        ("max_age_secs", Schema::Integer),
                    ]),
                ),
            ]);
            app.config_section(
                "server",
                crate::config::ServerConfig::default(),
                schema,
                validate_http_config,
            )?;
            app.instance(Arc::new(health::ServerReadiness::default()))
                .await?;
            app.instance(Arc::new(HttpServerInstalled)).await?;
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut BootContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config: crate::config::ServerConfig = app.section("server")?;
            validate_http_config(&config)?;
            validate_trusted_proxy_ranges(config.trusted_proxy_ranges.as_deref())?;
            crate::http::proxy::validate_headers(config.trusted_proxy_headers)?;
            let _ = app.resolve::<health::ServerReadiness>().await?;
            app.command(ServeCommand);
            app.command(StartCommand);
            crate::foundation::http::register_application_routes(
                app,
                config.metrics_endpoint_enabled,
            )?;
            Ok(())
        })
    }
}

fn validate_http_config(config: &crate::config::ServerConfig) -> Result<()> {
    if !(1..=3600).contains(&config.shutdown_timeout_secs) {
        return Err(crate::config::ConfigError::Invalid(
            "server.shutdown_timeout_secs must be 1..=3600 seconds".into(),
        )
        .into());
    }
    if !(1..=65_536).contains(&config.max_concurrent_requests) {
        return Err(crate::config::ConfigError::Invalid(
            "server.max_concurrent_requests must be 1..=65536".into(),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    #[tokio::test]
    async fn http_provider_contributes_health_and_command() -> Result<()> {
        let mut providers = crate::foundation::ProviderBuilder::new();
        providers.provider(HttpServiceProvider);
        let runtime = providers.start(Figment::new()).await?;
        assert!(
            runtime
                .container()
                .resolve::<health::ServerReadiness>()
                .await
                .is_ok()
        );
        assert_eq!(
            runtime
                .extension::<RouteRegistry>()
                .map(|routes| routes.len()),
            Some(1)
        );
        assert!(runtime.commands().any(|command| command.name() == "serve"));
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn http_provider_rejects_invalid_server_config() {
        let mut providers = crate::foundation::ProviderBuilder::new();
        providers.provider(HttpServiceProvider);
        let sources = Figment::from(Serialized::defaults(
            json!({"server":{"max_concurrent_requests":0}}),
        ));
        assert!(providers.start(sources).await.is_err());
    }
}

/// HTTP Provider 已安装的内部能力标记。
#[derive(Clone, Copy)]
pub(crate) struct HttpServerInstalled;
