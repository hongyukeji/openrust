//! 框架默认 Provider 清单，应用清单只登记应用自身服务。

use crate::foundation::ProviderId;
use crate::support::ServiceProvider;

struct ReadinessProvider;

impl ServiceProvider for ReadinessProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, crate::core::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(crate::core::Readiness::default()))
                .await?;
            Ok(())
        })
    }
}

/// 框架默认 Provider 集合；顺序即注册与启动顺序。
///
/// 与 Laravel `DefaultProviders` 一样，应用可在启动前追加、替换或排除具体项。
/// `ProviderBuilder::new()` 保持空清单，测试可显式选取所需服务。
pub struct DefaultProviders {
    selection: Selection,
    providers: Vec<Box<dyn ServiceProvider>>,
    actions: Vec<Action>,
}

pub(crate) struct ResolvedDefaultProviders {
    pub(crate) providers: Vec<Box<dyn ServiceProvider>>,
    pub(crate) database: Option<bool>,
}

enum Selection {
    Auto,
    Explicit,
}

enum Action {
    Merge(Vec<Box<dyn ServiceProvider>>),
    Replace(ProviderId, Box<dyn ServiceProvider>),
    Except(std::collections::HashSet<ProviderId>),
}

impl DefaultProviders {
    /// 显式空清单，供只测试指定 Provider 的嵌入式应用使用。
    pub fn empty() -> Self {
        Self {
            selection: Selection::Explicit,
            providers: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// 从本次已加载的唯一配置仓储选择默认项。
    pub fn auto() -> Self {
        Self {
            selection: Selection::Auto,
            providers: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// 按启动配置选用需要数据库或 Session 的默认项。
    pub fn for_runtime(database: bool, session: bool) -> Self {
        Self::build(database, session)
    }

    fn build(database: bool, session: bool) -> Self {
        let mut providers: Vec<Box<dyn ServiceProvider>> = vec![
            Box::new(crate::http::HttpServiceProvider),
            Box::new(ReadinessProvider),
            Box::new(crate::translation::TranslationServiceProvider::default()),
            Box::new(crate::mail::MailerServiceProvider),
            Box::new(crate::sms::SmsServiceProvider),
            Box::new(crate::cache::CacheServiceProvider::new()),
        ];
        #[cfg(feature = "hashing")]
        providers.push(Box::new(crate::hashing::HashServiceProvider::new()));
        #[cfg(feature = "encryption")]
        providers.push(Box::new(crate::encryption::EncryptionServiceProvider));
        #[cfg(feature = "encryption")]
        providers.push(Box::new(crate::http::CookieServiceProvider::new()));
        #[cfg(not(feature = "encryption"))]
        providers.push(Box::new(crate::http::CookieServiceProvider::plain()));
        #[cfg(feature = "storage-local")]
        providers.push(Box::new(crate::filesystem::FilesystemServiceProvider::new()));
        #[cfg(feature = "view")]
        providers.push(Box::new(crate::view::OptionalViewServiceProvider));
        providers.push(Box::new(crate::database::DatabaseServiceProvider));
        #[cfg(feature = "session")]
        if session {
            providers.push(Box::new(crate::session::SessionServiceProvider::new()));
        }
        #[cfg(not(feature = "session"))]
        let _ = session;
        #[cfg(feature = "queue")]
        {
            providers.push(Box::new(crate::queue::QueueServiceProvider::new()));
            providers.push(Box::new(crate::notifications::NotificationsServiceProvider));
        }
        if database {
            #[cfg(feature = "auth")]
            {
                providers.push(Box::new(crate::auth::JwtServiceProvider));
                providers.push(Box::new(crate::auth::AuthServiceProvider::new()));
                providers.push(Box::new(crate::audit::AuditServiceProvider::database()));
                providers.push(Box::new(crate::auth::AuthFlowsServiceProvider::new()));
                #[cfg(feature = "auth-fortify")]
                providers.push(Box::new(crate::auth::fortify::FortifyServiceProvider::new()));
            }
            #[cfg(feature = "signing")]
            providers.push(Box::new(
                crate::signing::SignedLinksServiceProvider::new()
                    .with_database_revocations_from_app(),
            ));
        }
        Self {
            selection: Selection::Explicit,
            providers,
            actions: Vec::new(),
        }
    }

    /// 追加显式 Provider；同类型去重与强制重注册由统一启动管线处理。
    #[must_use]
    pub fn merge(mut self, providers: impl IntoIterator<Item = Box<dyn ServiceProvider>>) -> Self {
        self.actions
            .push(Action::Merge(providers.into_iter().collect()));
        self
    }

    /// 在原清单槽位替换一个具体 Provider；缺失时不修改清单。
    #[must_use]
    pub fn replace(mut self, from: ProviderId, to: Box<dyn ServiceProvider>) -> Self {
        self.actions.push(Action::Replace(from, to));
        self
    }

    /// 从默认清单排除具体 Provider 类型。
    #[must_use]
    pub fn except(mut self, ids: impl IntoIterator<Item = ProviderId>) -> Self {
        self.actions.push(Action::Except(ids.into_iter().collect()));
        self
    }

    /// 将清单交给唯一 Provider 启动器。
    pub fn resolve(
        self,
        config: &crate::config::repository::ConfigRepository,
    ) -> crate::core::Result<Vec<Box<dyn ServiceProvider>>> {
        self.resolve_selection(config)
            .map(|selected| selected.providers)
    }

    pub(crate) fn resolve_selection(
        self,
        config: &crate::config::repository::ConfigRepository,
    ) -> crate::core::Result<ResolvedDefaultProviders> {
        let mut automatic_database = None;
        let mut providers = match self.selection {
            Selection::Auto => {
                let database = has_default_database(config)?;
                automatic_database = Some(database);
                let session = config
                    .get("session.driver")?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .is_some();
                Self::build(database, session).providers
            }
            Selection::Explicit => Vec::new(),
        };
        providers.extend(self.providers);
        for action in self.actions {
            match action {
                Action::Merge(items) => providers.extend(items),
                Action::Replace(from, to) => {
                    if let Some(index) = providers.iter().position(|item| item.id() == from) {
                        providers[index] = to;
                    }
                }
                Action::Except(ids) => providers.retain(|provider| !ids.contains(&provider.id())),
            }
        }
        Ok(ResolvedDefaultProviders {
            providers,
            database: automatic_database,
        })
    }
}

fn has_default_database(
    config: &crate::config::repository::ConfigRepository,
) -> crate::core::Result<bool> {
    let database = if config.get("database")?.is_some() {
        config.section::<crate::config::DatabaseConfig>("database")?
    } else {
        crate::config::DatabaseConfig::default()
    };
    Ok(database.has_default_connection())
}

pub(crate) fn validate_database_selection(
    config: &crate::config::repository::ConfigRepository,
    selected: Option<bool>,
) -> crate::core::Result<()> {
    if let Some(selected) = selected
        && selected != has_default_database(config)?
    {
        return Err(crate::core::Error::custom(
            "database_selection_changed",
            "自动 Provider 选择后默认数据库启用状态发生变化，请以最终配置重新引导应用",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Replacement;
    impl ServiceProvider for Replacement {}

    #[test]
    fn default_list_can_replace_except_and_merge_without_changing_other_slots()
    -> crate::core::Result<()> {
        let repository = crate::config::repository::ConfigRepository::new(
            crate::config::ConfigSources::from_ordered_entries([("app", json!({"name": "test"}))])?,
        );
        let original = DefaultProviders::for_runtime(false, false).resolve(&repository)?;
        let old = original
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>();
        let target = ProviderId::of::<crate::http::HttpServiceProvider>();
        let position = old.iter().position(|id| *id == target);
        assert!(position.is_some());
        let edited = DefaultProviders::for_runtime(false, false)
            .replace(target, Box::new(Replacement))
            .except([ProviderId::of::<crate::mail::MailerServiceProvider>()])
            .merge([Box::new(crate::mail::MailerServiceProvider) as Box<dyn ServiceProvider>])
            .resolve(&repository)?;
        let ids = edited
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>();
        assert_eq!(
            ids.get(position.unwrap_or_default()),
            Some(&ProviderId::of::<Replacement>())
        );
        assert_eq!(
            ids.last(),
            Some(&ProviderId::of::<crate::mail::MailerServiceProvider>())
        );
        Ok(())
    }

    struct DatabaseSelectionChange {
        value: serde_json::Value,
        during_register: bool,
    }

    impl ServiceProvider for DatabaseSelectionChange {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                if self.during_register {
                    app.config_repository()
                        .set("database.url", self.value.clone())?;
                }
                Ok(())
            })
        }

        fn boot<'a>(
            &'a self,
            app: &'a mut crate::foundation::BootContext<'_>,
        ) -> crate::support::BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                if !self.during_register {
                    app.config_repository()
                        .set("database.url", self.value.clone())?;
                }
                Ok(())
            })
        }
    }

    async fn changed_database_selection_fails(
        initial: serde_json::Value,
        next: serde_json::Value,
    ) -> crate::core::Result<()> {
        for during_register in [true, false] {
            let mut builder = crate::foundation::ProviderBuilder::new();
            // 前序业务 Provider 使用同仓储 set；自动默认清单仍在任何 register 前选择。
            builder
                .provider(DatabaseSelectionChange {
                    value: next.clone(),
                    during_register,
                })
                .default_providers(DefaultProviders::auto());
            let source = figment::Figment::from(figment::providers::Serialized::defaults(
                crate::config::FrameworkConfig::default()
            )).merge(figment::providers::Serialized::defaults(json!({
                "database":{"url":initial},
                "cache":{"default":"memory","stores":{"memory":{"driver":"memory"}}},
                "auth":{"jwt":{"issuer":"http://fixture.test","audience":"fixture","active_key_id":"test","keys":{
                    "test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
                }}},
                "security":{
                    "encryption":{"active_key_id":"test","keys":{
                        "test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
                    }},
                    "signed_links":{"issuer":"http://fixture.test","active_key_id":"test","keys":{
                        "test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
                    }}
                }
            })));
            let error = match builder.start(source).await {
                Ok(runtime) => {
                    runtime
                        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                        .await?;
                    return Err(crate::core::Error::custom(
                        "test_selection_change_accepted",
                        "自动清单接受了默认连接选择变化",
                    ));
                }
                Err(error) => error,
            };
            assert_eq!(error.code(), "database_selection_changed");
        }
        Ok(())
    }

    #[tokio::test]
    async fn auto_selection_rejects_none_to_some_in_register_and_boot() -> crate::core::Result<()> {
        changed_database_selection_fails(
            serde_json::Value::Null,
            json!("unknown://private.invalid/db"),
        )
        .await
    }

    #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
    #[tokio::test]
    async fn auto_selection_rejects_some_to_none_before_opening_a_pool() -> crate::core::Result<()>
    {
        let url = if cfg!(feature = "db-postgres") {
            "postgres://private.invalid/db"
        } else {
            "mysql://private.invalid/db"
        };
        changed_database_selection_fails(json!(url), serde_json::Value::Null).await
    }

    #[test]
    fn automatic_selection_rejects_invalid_database_types_without_echoing_values()
    -> crate::core::Result<()> {
        let repository = crate::config::repository::ConfigRepository::new(
            crate::config::ConfigSources::from_ordered_entries([(
                "database",
                json!({
                    "url": {"private-key":"private-value"}
                }),
            )])?,
        );
        let error = DefaultProviders::auto()
            .resolve(&repository)
            .err()
            .ok_or_else(|| {
                crate::core::Error::custom("test_expected_config_error", "非法数据库类型没有拒绝")
            })?;
        assert!(!error.to_string().contains("private-key"));
        assert!(!error.to_string().contains("private-value"));
        Ok(())
    }

    #[test]
    fn named_only_database_keeps_auth_gate_and_selects_configured_session()
    -> crate::core::Result<()> {
        let repository = crate::config::repository::ConfigRepository::new(
            crate::config::ConfigSources::from_ordered_entries([
                (
                    "database",
                    json!({"instances":{"reporting":{"url":"mysql://example.invalid/reporting"}}}),
                ),
                (
                    "session",
                    json!({"driver":"database", "connection":"reporting"}),
                ),
            ])?,
        );
        let ids = DefaultProviders::auto()
            .resolve(&repository)?
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>();
        assert!(ids.contains(&ProviderId::of::<crate::database::DatabaseServiceProvider>()));
        #[cfg(feature = "auth")]
        assert!(!ids.contains(&ProviderId::of::<crate::auth::AuthServiceProvider>()));
        #[cfg(feature = "queue")]
        assert!(ids.contains(&ProviderId::of::<crate::queue::QueueServiceProvider>()));
        #[cfg(feature = "session")]
        assert!(ids.contains(&ProviderId::of::<crate::session::SessionServiceProvider>()));
        Ok(())
    }

    #[test]
    fn automatic_selection_uses_loaded_database_and_session_values() -> crate::core::Result<()> {
        let repository = crate::config::repository::ConfigRepository::new(
            crate::config::ConfigSources::from_ordered_entries([
                ("database", json!({"url": "postgres://example.invalid/db"})),
                ("session", json!({"driver": "memory"})),
            ])?,
        );
        let ids = DefaultProviders::auto()
            .resolve(&repository)?
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>();
        #[cfg(feature = "session")]
        assert!(ids.contains(&ProviderId::of::<crate::session::SessionServiceProvider>()));
        #[cfg(feature = "auth")]
        assert!(ids.contains(&ProviderId::of::<crate::auth::AuthServiceProvider>()));
        #[cfg(not(any(feature = "session", feature = "auth")))]
        let _ = ids;
        Ok(())
    }
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    macro_rules! trace_provider {
        ($name:ident, $label:literal) => {
            struct $name(Arc<Mutex<Vec<&'static str>>>);
            impl ServiceProvider for $name {
                fn register<'a>(
                    &'a self,
                    _app: &'a mut crate::foundation::RegisterContext<'_>,
                ) -> crate::support::BoxFuture<'a, crate::core::Result<()>> {
                    Box::pin(async move {
                        self.0
                            .lock()
                            .map_err(|_| {
                                crate::core::Error::internal(std::io::Error::other("trace lock"))
                            })?
                            .push($label);
                        Ok(())
                    })
                }
            }
        };
    }
    trace_provider!(FrameworkTrace, "framework");
    trace_provider!(PackageTrace, "package");
    trace_provider!(ApplicationTrace, "application");

    #[tokio::test]
    async fn default_package_application_register_in_order() -> crate::core::Result<()> {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.default_providers(
            DefaultProviders::empty()
                .merge([Box::new(FrameworkTrace(Arc::clone(&trace))) as Box<dyn ServiceProvider>]),
        );
        builder.provider(PackageTrace(Arc::clone(&trace)));
        builder.provider(ApplicationTrace(Arc::clone(&trace)));
        let source = figment::Figment::from(figment::providers::Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        ));
        let runtime = builder.start(source).await?;
        let observed = trace
            .lock()
            .map_err(|_| crate::core::Error::internal(std::io::Error::other("trace lock")))?
            .clone();
        assert_eq!(observed, ["framework", "package", "application"]);
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }
}
