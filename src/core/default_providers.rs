//! 框架默认 Provider 清单，应用清单只登记应用自身服务。

use super::{ProviderId, ServiceProvider};

struct ReadinessProvider;

impl ServiceProvider for ReadinessProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut super::RegisterContext<'_>,
    ) -> super::BoxFuture<'a, super::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(super::Readiness::default()))
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
            Box::new(crate::i18n::I18nServiceProvider::default()),
            Box::new(crate::mail::MailerServiceProvider),
            Box::new(crate::sms::SmsServiceProvider),
            Box::new(crate::cache::CacheServiceProvider::new()),
        ];
        #[cfg(feature = "encryption")]
        providers.push(Box::new(crate::security::EncryptionServiceProvider));
        #[cfg(feature = "encryption")]
        providers.push(Box::new(crate::http::CookieServiceProvider::new()));
        #[cfg(not(feature = "encryption"))]
        providers.push(Box::new(crate::http::CookieServiceProvider::plain()));
        #[cfg(feature = "storage-local")]
        providers.push(Box::new(crate::storage::StorageServiceProvider::new()));
        #[cfg(feature = "view")]
        providers.push(Box::new(crate::view::OptionalViewServiceProvider));
        providers.push(Box::new(crate::db::DatabaseServiceProvider));
        #[cfg(feature = "session")]
        if session {
            providers.push(Box::new(crate::session::SessionServiceProvider::new()));
        }
        #[cfg(not(feature = "session"))]
        let _ = session;
        if database {
            #[cfg(feature = "auth")]
            {
                providers.push(Box::new(crate::auth::JwtServiceProvider));
                providers.push(Box::new(crate::auth::AuthServiceProvider::new()));
                providers.push(Box::new(crate::audit::AuditServiceProvider::database()));
                providers.push(Box::new(crate::auth::AuthFlowsServiceProvider::new()));
                #[cfg(feature = "auth-mfa")]
                providers.push(Box::new(crate::auth::MfaServiceProvider));
            }
            #[cfg(feature = "signing")]
            providers.push(Box::new(
                crate::signing::SignedLinksServiceProvider::new()
                    .with_database_revocations_from_app(),
            ));
            #[cfg(feature = "queue")]
            {
                providers.push(Box::new(crate::queue::QueueServiceProvider::new()));
                providers.push(Box::new(crate::notifications::NotificationsServiceProvider));
            }
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
        let mut providers = match self.selection {
            Selection::Auto => {
                let database = config
                    .get("database.url")?
                    .is_some_and(|value| value.as_str().is_some())
                    || config.get("database.instances")?.is_some_and(|value| {
                        value.as_object().is_some_and(|items| !items.is_empty())
                    })
                    || config.get("database.read_replicas")?.is_some_and(|value| {
                        value.as_array().is_some_and(|items| !items.is_empty())
                    });
                let session = config
                    .get("session.driver")?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .is_some_and(|driver| driver != "database" || database);
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
        Ok(providers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Replacement;
    impl ServiceProvider for Replacement {}

    #[test]
    fn default_list_can_replace_except_and_merge_without_changing_other_slots()
    -> super::super::Result<()> {
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

    #[test]
    fn automatic_selection_uses_loaded_database_and_session_values() -> super::super::Result<()> {
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
                    _app: &'a mut super::super::RegisterContext<'_>,
                ) -> super::super::BoxFuture<'a, super::super::Result<()>> {
                    Box::pin(async move {
                        self.0
                            .lock()
                            .map_err(|_| {
                                super::super::Error::internal(std::io::Error::other("trace lock"))
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
    async fn default_package_application_register_in_order() -> super::super::Result<()> {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut builder = super::super::ProviderBuilder::new();
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
            .map_err(|_| super::super::Error::internal(std::io::Error::other("trace lock")))?
            .clone();
        assert_eq!(observed, ["framework", "package", "application"]);
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }
}
