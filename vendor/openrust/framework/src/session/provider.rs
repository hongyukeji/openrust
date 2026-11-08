//! 唯一Session状态的Provider、最终配置和请求层。

use super::{
    CookieSessionStore, DatabaseSessionStore, GuardedDatabaseSessionStore, MemorySessionStore,
    Session, SessionStore, SessionStoreGuarantees,
};
use crate::Error;
use crate::Result;
use crate::config::SessionConfig;
use crate::foundation::BootContext;
use crate::foundation::RegisterContext;
use crate::support::{BoxFuture, ServiceProvider};
use std::sync::{Arc, OnceLock};

/// 共享store撤销句柄；所有认证和普通Session使用同一driver。
#[derive(Clone)]
pub struct SessionStoreHandle(pub(crate) Arc<dyn SessionStore>);
impl std::fmt::Debug for SessionStoreHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionStoreHandle([REDACTED])")
    }
}
impl SessionStoreHandle {
    /// 使用应用公开实现的store。
    pub fn new(store: impl SessionStore + 'static) -> Self {
        Self(Arc::new(store))
    }
    /// 克隆当前绑定的store trait句柄，供受信应用Provider装饰既有绑定。
    ///
    /// 这是底层逃生口：直接调用会绕过受管理Session的状态、容量检查，
    /// 也会绕过本句柄`delete`的ServerSide保证检查。调用者须自行遵守
    /// 并发、容量和撤销契约；客户端快照不能因此获得全局撤销能力。
    /// 克隆不会复制或重建后端。
    pub fn store(&self) -> Arc<dyn SessionStore> {
        Arc::clone(&self.0)
    }
    /// 后端的保证分类；不读取请求内数据。
    pub fn guarantees(&self) -> SessionStoreGuarantees {
        self.0.guarantees()
    }
    /// 删除指定会话；浏览器快照不能提供跨请求撤销，明确返回错误。
    pub async fn delete(&self, id: &str) -> Result<()> {
        if self.guarantees() != SessionStoreGuarantees::ServerSide {
            return Err(super::store::unsupported_guarantee());
        }
        self.0.delete(&super::SessionId::parse(id)?).await
    }
}
#[derive(Clone)]
pub(crate) struct SessionSettings {
    pub(crate) config: SessionConfig,
    pub(crate) cookie_name: String,
}
impl SessionSettings {
    async fn configured_store(
        &self,
        resolver: &crate::container::Resolver,
    ) -> Result<Arc<dyn SessionStore>> {
        let store: Arc<dyn SessionStore> = match self.config.driver.as_deref() {
            Some(driver @ ("database" | "database-guarded")) => {
                let db = if let Some(connection) = &self.config.connection {
                    resolver
                        .resolve::<crate::database::Databases>()
                        .await?
                        .get(connection)?
                } else {
                    (*resolver.resolve::<crate::database::Db>().await?).clone()
                };
                if driver == "database" {
                    let store = DatabaseSessionStore::new(db)
                        .configured(self.config.table.clone(), self.config.lifetime)?;
                    #[cfg(feature = "auth")]
                    let store = store.with_session_user_metadata(
                        resolver.is_bound::<crate::auth::AuthService>()?,
                    );
                    Arc::new(store)
                } else {
                    Arc::new(
                        GuardedDatabaseSessionStore::new(db)
                            .for_table(self.config.table.clone())?,
                    )
                }
            }
            Some("array") => Arc::new(MemorySessionStore::default()),
            Some("cookie") => Arc::new(CookieSessionStore::new(
                self.config.lifetime,
                self.config.expire_on_close,
            )?),
            _ => {
                return Err(Error::custom(
                    "session_config_invalid",
                    "Session driver要求公开with_store实现",
                ));
            }
        };
        Ok(store)
    }

    fn read(repository: &crate::config::repository::ConfigRepository) -> Result<Self> {
        let config: SessionConfig = repository.section("session")?;
        let same_site = config.validate()?;
        time::OffsetDateTime::now_utc()
            .checked_add(time::Duration::seconds(config.lifetime_seconds()?))
            .ok_or_else(|| {
                Error::Config(crate::config::ConfigError::Invalid(
                    "session.lifetime: 超出可表示的时间范围".into(),
                ))
            })?;
        let cookie_name = if let Some(name) = &config.cookie {
            name.clone()
        } else {
            let app: crate::config::AppConfig = repository.section("app")?;
            super::name::default_cookie_name(&app.name)?
        };
        let mut factory = crate::http::CookieFactory::default();
        factory.set_default_path_and_domain(
            config.path.clone(),
            config.domain.clone(),
            config.secure,
            same_site,
        )?;
        factory
            .make(
                cookie_name.clone(),
                "a".repeat(40),
                if config.expire_on_close {
                    0
                } else {
                    i64::try_from(config.lifetime).map_err(Error::internal)?
                },
                crate::http::CookieOptions::default(),
            )?
            .to_header(false)?;
        Ok(Self {
            config,
            cookie_name,
        })
    }
}
#[derive(Default)]
pub(crate) struct SessionSlot(pub(crate) OnceLock<Session>);
#[derive(Clone, Default)]
struct Ready(Arc<OnceLock<Option<super::middleware::SessionLayer>>>);
/// 最终 Session 层由容器启动后锁定，供 web 路由组按请求安装。
#[derive(Clone)]
pub(crate) struct SessionRuntime(Arc<OnceLock<Option<super::middleware::SessionLayer>>>);
impl SessionRuntime {
    pub(crate) fn layer(&self) -> Result<Option<super::middleware::SessionLayer>> {
        self.0.get().cloned().ok_or_else(missing)
    }
}
struct Installed;

/// 安装立即启动、逐响应保存的Session生命周期；默认database。
pub struct SessionServiceProvider {
    config: Option<SessionConfig>,
    store: Option<Arc<dyn SessionStore>>,
    error_bag_codecs: Arc<super::ErrorBagCodecs>,
}
impl Default for SessionServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}
impl SessionServiceProvider {
    /// 从最终配置选择同一持久driver；不会隐式降级内存。
    pub fn new() -> Self {
        Self {
            config: None,
            store: None,
            error_bag_codecs: Arc::new(super::ErrorBagCodecs::new()),
        }
    }
    /// 安装应用公开实现的store，优先于配置driver。
    pub fn with_store(store: impl SessionStore + 'static) -> Self {
        Self {
            config: None,
            store: Some(Arc::new(store)),
            error_bag_codecs: Arc::new(super::ErrorBagCodecs::new()),
        }
    }
    /// 装配已登记的错误包 codec；注册后不可在请求期变更。
    #[must_use]
    pub fn with_error_bag_codecs(mut self, codecs: super::ErrorBagCodecs) -> Self {
        self.error_bag_codecs = Arc::new(codecs);
        self
    }
    /// 在本Provider boot时覆盖唯一session区段；更晚overlay仍按通常顺序生效。
    #[must_use]
    pub fn with_cookie_config(mut self, config: SessionConfig) -> Self {
        self.config = Some(config);
        self
    }
}
impl ServiceProvider for SessionServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            app.instance(Arc::clone(&self.error_bag_codecs)).await?;
            app.config_section(
                "session",
                SessionConfig::default(),
                S::object([
                    ("driver", S::String.nullable()),
                    ("connection", S::String.nullable()),
                    ("table", S::String),
                    ("cookie", S::String.nullable()),
                    ("path", S::String),
                    ("domain", S::String.nullable()),
                    ("secure", S::Boolean.nullable()),
                    ("http_only", S::Boolean),
                    ("same_site", S::String.nullable()),
                    ("lifetime", S::Integer),
                    ("lottery", S::array(S::Integer)),
                    ("expire_on_close", S::Boolean),
                    ("partitioned", S::Boolean),
                    ("max_bytes", S::Integer),
                    ("serializer", S::String),
                ]),
                |_: &SessionConfig| Ok(()),
            )?;
            let repository = app.config_repository();
            app.singleton::<SessionSettings, _>(move |_| {
                let repository = repository.clone();
                Box::pin(async move { Ok(Arc::new(SessionSettings::read(&repository)?)) })
            })
            .await?;
            let custom = self.store.clone();
            app.singleton::<SessionStoreHandle, _>(move |resolver| {
                let custom = custom.clone();
                Box::pin(async move {
                    let settings = resolver.resolve::<SessionSettings>().await?;
                    let store: Arc<dyn SessionStore> = if let Some(store) = custom {
                        store
                    } else {
                        settings.configured_store(&resolver).await?
                    };
                    Ok(Arc::new(SessionStoreHandle(store)))
                })
            })
            .await?;
            app.scoped::<SessionSlot, _>(|_| {
                Box::pin(async { Ok(Arc::new(SessionSlot::default())) })
            })
            .await?;
            app.scoped::<Session, _>(|resolver| {
                Box::pin(async move {
                    let slot = resolver.resolve::<SessionSlot>().await?;
                    Ok(Arc::new(slot.0.get().cloned().ok_or_else(missing)?))
                })
            })
            .await?;
            let ready = Ready::default();
            app.instance(Arc::new(ready.clone())).await?;
            app.singleton::<Installed, _>(move |resolver| {
                let ready = ready.clone();
                Box::pin(async move {
                    let settings = resolver.resolve::<SessionSettings>().await?;
                    let error_bag_codecs = resolver.resolve::<super::ErrorBagCodecs>().await?;
                    let layer = if settings.config.driver.is_some() {
                        let store = resolver.resolve::<SessionStoreHandle>().await?;
                        Some(super::middleware::SessionLayer {
                            settings: (*settings).clone(),
                            store: store.0.clone(),
                            error_bag_codecs,
                        })
                    } else {
                        None
                    };
                    ready.0.set(layer).map_err(|_| {
                        Error::custom_with_status(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            "session_layer_invalid",
                            "Session层重复装配",
                        )
                    })?;
                    Ok(Arc::new(Installed))
                })
            })
            .await?;
            app.require::<Installed>();
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if let Some(config) = &self.config {
                app.overlay(
                    "session",
                    serde_json::to_value(config).map_err(Error::internal)?,
                )?;
            }
            // 语义只校验最终快照，允许后续 Provider 修复类型正确的中间值。
            let repository = app.config_repository();
            let custom_store = self.store.is_some();
            app.validate_at_finalize(move || {
                Box::pin(async move {
                    let settings = SessionSettings::read(&repository)?;
                    if !custom_store
                        && settings.config.driver.as_deref().is_some_and(|driver| {
                            !matches!(driver, "database" | "database-guarded" | "array" | "cookie")
                        })
                    {
                        return Err(Error::Config(crate::config::ConfigError::Invalid(
                            "session.driver: 需要公开 with_store 实现".into(),
                        )));
                    }
                    Ok(())
                })
            });
            let ready = app.resolve::<Ready>().await?;
            app.extension_mut::<Option<SessionRuntime>>()?
                .replace(SessionRuntime(ready.0.clone()));
            Ok(())
        })
    }
}
pub(crate) fn missing() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_layer_missing",
        "Session要求SessionServiceProvider及受管理请求作用域",
    )
}
