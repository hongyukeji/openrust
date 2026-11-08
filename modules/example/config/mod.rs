//! 示例模块的可编辑 Rust 配置函数。

use openrust::config::{ConfigContext, ConfigError};

/// 与应用共享同一个 `ConfigRepository` 的模块区段。
pub(crate) fn boot() -> openrust::config::ConfigManifest {
    openrust::config::ConfigManifest::new().section("example", config)
}

/// 只有显式启用策略演示时才登记无法缓存的进程本地工厂。
pub(crate) fn runtime_policy_manifest() -> openrust::config::ConfigManifest {
    openrust::config::ConfigManifest::new().typed("example.runtime_policy", runtime_policy)
}

/// 只控制本文件是否提供可信主机默认策略；显式宿主来源仍可独立配置。
const TRUST_HOSTS_ENABLED: bool = false;

/// 在应用同次 dotenv 快照上创建模块配置。
pub(crate) fn config(ctx: &ConfigContext) -> Result<serde_json::Value, ConfigError> {
    // None 表示此文件不覆盖宿主策略；需要主动清空时使用显式来源的 null。
    // 改为 Some("https://summary.example.invalid".to_owned()) 可启用上游摘要来源。
    let article_summary_origin: Option<String> = None;
    // 将开关改为 true 后可编辑完整 Some 策略；本模块投影的 at=None 继承宿主列表。
    let trusted_hosts = if TRUST_HOSTS_ENABLED {
        Some(openrust::config::TrustedHostsConfig {
            at: None,
            subdomains: true,
        })
    } else {
        None
    };
    // 只投影本文件明确提供的值，避免 optional=null 抹掉宿主已选策略。
    let mut values = serde_json::json!({
        "greeting": ctx.env_or("EXAMPLE_GREETING", "Hello from modules/example"),
        "feature": {"enabled": ctx.bool_or("EXAMPLE_FEATURE_ENABLED", true)?},
    });
    if ctx.env("EXAMPLE_LOGGING_DEMO").is_some() {
        values["logging_demo"] = ctx.bool_or("EXAMPLE_LOGGING_DEMO", false)?.into();
    }
    if let Some(suffix) = ctx.env("EXAMPLE_GREETING_SUFFIX") {
        values["greeting_suffix"] = serde_json::Value::String(suffix);
    }
    if let Some(origin) = article_summary_origin {
        values["article_summary_origin"] = serde_json::Value::String(origin);
    }
    if let Some(hosts) = trusted_hosts {
        values["trusted_hosts"] = trusted_hosts_values(hosts);
    }
    #[cfg(feature = "auth-oidc")]
    if let Some(oidc) = oidc(ctx)? {
        values["oidc"] = oidc;
    }
    Ok(values)
}

/// 仅在启用模块时构造的进程本地策略值；由 Provider 消费。
pub(crate) struct ExampleRuntimePolicy {
    pub(crate) greeting_prefix: String,
}

/// typed 配置工厂不能写入配置缓存，需在每次引导时重新求值。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest typed 工厂统一的可失败契约"
)]
pub(crate) fn runtime_policy(ctx: &ConfigContext) -> Result<ExampleRuntimePolicy, ConfigError> {
    Ok(ExampleRuntimePolicy {
        greeting_prefix: ctx.env_or("EXAMPLE_GREETING_PREFIX", "Example"),
    })
}

/// 只有显式启用时提供 OIDC 策略；不序列化会跳过 `client_secret` 的框架业务类型。
#[cfg(feature = "auth-oidc")]
fn oidc(ctx: &ConfigContext) -> Result<Option<serde_json::Value>, ConfigError> {
    const ENABLED: bool = false;
    if !ENABLED {
        return Ok(None);
    }
    let app_url = ctx.require::<String>("app.url")?;
    Ok(Some(oidc_values(
        super::services::oidc::ExampleOidcConfig {
            // issuer 是外部身份提供方，与 app.url 语义独立；启用前填写真实部署值。
            issuer: ctx.env_or("EXAMPLE_OIDC_ISSUER", "https://identity.example.invalid"),
            client_id: ctx.env_or("EXAMPLE_OIDC_CLIENT_ID", ""),
            // None 在本模块默认投影中继承宿主秘密，Some（含空串）明确提供字段。
            client_secret: ctx.env("EXAMPLE_OIDC_CLIENT_SECRET"),
            redirect_uri: ctx.env_or(
                "EXAMPLE_OIDC_REDIRECT_URI",
                &format!(
                    "{}/api/example/oidc/callback",
                    app_url.trim_end_matches('/')
                ),
            ),
            // 仅显式本地测试允许回环 HTTP；部署默认必须使用 HTTPS。
            allow_loopback: false,
        },
    )))
}

// 仅此模块的默认投影省略 None 以继承宿主；不改变 Config 中 null 的实际覆盖语义。
// 框架 TrustedHostsConfig::at=None 仍表示从 app.url 推导；需要显式重置时由宿主来源
// 或公开扩展提供该值，模块默认不发送 at:null。
fn trusted_hosts_values(hosts: openrust::config::TrustedHostsConfig) -> serde_json::Value {
    let mut values = serde_json::json!({"subdomains": hosts.subdomains});
    if let Some(at) = hosts.at {
        values["at"] = serde_json::json!(at);
    }
    values
}

#[cfg(feature = "auth-oidc")]
fn oidc_values(oidc: super::services::oidc::ExampleOidcConfig) -> serde_json::Value {
    let mut values = serde_json::json!({
        "issuer": oidc.issuer,
        "client_id": oidc.client_id,
        "redirect_uri": oidc.redirect_uri,
        "allow_loopback": oidc.allow_loopback,
    });
    // 不用业务类型的 Serialize（会跳过秘密），也不将 None 投影成覆盖宿主的 null。
    if let Some(secret) = oidc.client_secret {
        values["client_secret"] = serde_json::Value::String(secret);
    }
    values
}

#[cfg(test)]
mod projection_tests {
    use openrust::config::{ConfigLoader, ConfigManifest, repository::SectionSchema};
    use openrust::foundation::BootContext;
    use openrust::foundation::ProviderBuilder;
    use openrust::foundation::RegisterContext;
    use openrust::support::ServiceProvider;
    use serde_json::{Value, json};

    struct PolicyProjection(Value);

    impl ServiceProvider for PolicyProjection {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
            Box::pin(async move {
                app.config_section("example", json!({}), SectionSchema::any(), |_| Ok(()))
            })
        }

        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
            let values = self.0.clone();
            Box::pin(async move {
                app.code_config(
                    &ConfigManifest::new().section("example", move |_| Ok(values.clone())),
                )
            })
        }
    }

    // 经真实 BootContext::code_config 递归应用，而非用测试自建合并算法代替生产链。
    async fn apply_projection(host: Value, module: Value) -> openrust::Result<Value> {
        let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
        let sources = ConfigLoader::application_code_sources_with_overrides(
            root.path(),
            "PROJECTION_",
            None,
            &ConfigManifest::new(),
            [("example", host)],
        )?;
        let mut builder = ProviderBuilder::new();
        builder.provider(PolicyProjection(module));
        let runtime = builder.start(sources).await?;
        let result = runtime.config().section("example")?;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        Ok(result)
    }

    #[tokio::test]
    async fn enabled_hosts_projection_inherits_absent_at_but_retains_explicit_empty_list()
    -> openrust::Result<()> {
        let inherited = super::trusted_hosts_values(openrust::config::TrustedHostsConfig {
            at: None,
            subdomains: false,
        });
        assert!(inherited.get("at").is_none());
        let final_values = apply_projection(
            json!({"trusted_hosts": {"at": ["^host\\.test$"], "subdomains": true}}),
            json!({"trusted_hosts": inherited}),
        )
        .await?;
        assert_eq!(
            final_values["trusted_hosts"]["at"],
            json!(["^host\\.test$"])
        );
        assert_eq!(final_values["trusted_hosts"]["subdomains"], false);
        let explicit = super::trusted_hosts_values(openrust::config::TrustedHostsConfig {
            at: Some(Vec::new()),
            subdomains: false,
        });
        // 此处只证明投影保留 []；递归数组合并规则仍由仓储决定，不暗示 [] 必然清空。
        assert_eq!(explicit.get("at"), Some(&json!([])));
        Ok(())
    }

    #[cfg(feature = "auth-oidc")]
    #[tokio::test]
    async fn enabled_oidc_projection_inherits_absent_secret_and_keeps_explicit_empty_string()
    -> openrust::Result<()> {
        for secret in [None, Some(String::new())] {
            let projected = super::oidc_values(super::super::services::oidc::ExampleOidcConfig {
                issuer: "https://identity.example.invalid".to_owned(),
                client_id: "module-client".to_owned(),
                client_secret: secret.clone(),
                redirect_uri: "https://app.example.invalid/api/example/oidc/callback".to_owned(),
                allow_loopback: false,
            });
            assert_eq!(
                projected.get("client_secret"),
                secret
                    .as_ref()
                    .map(|value| Value::String(value.clone()))
                    .as_ref()
            );
            let final_values = apply_projection(
                json!({"oidc": {"client_secret": "fixture-secret", "client_id": "host-client"}}),
                json!({"oidc": projected}),
            )
            .await?;
            assert_eq!(
                final_values["oidc"]["client_secret"],
                secret.as_deref().unwrap_or("fixture-secret")
            );
            assert_eq!(final_values["oidc"]["client_id"], "module-client");
        }
        Ok(())
    }
}
