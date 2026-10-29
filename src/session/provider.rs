//! 唯一Session状态的Provider、最终配置和请求层。

use super::{DatabaseSessionStore, MemorySessionStore, Session, SessionStore};
use crate::{
    Error, Result,
    config::SessionConfig,
    core::{BootContext, BoxFuture, RegisterContext, ServiceProvider},
};
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
    /// 删除指定会话；不向日志输出凭据。
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.0.delete(&super::SessionId::parse(id)?).await
    }
}
#[derive(Clone)]
pub(crate) struct SessionSettings {
    pub(crate) config: SessionConfig,
    pub(crate) cookie_name: String,
}
impl SessionSettings {
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
struct Installed;

/// 安装立即启动、逐响应保存的Session生命周期；默认database。
pub struct SessionServiceProvider {
    config: Option<SessionConfig>,
    store: Option<Arc<dyn SessionStore>>,
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
        }
    }
    /// 安装应用公开实现的store，优先于配置driver。
    pub fn with_store(store: impl SessionStore + 'static) -> Self {
        Self {
            config: None,
            store: Some(Arc::new(store)),
        }
    }
    /// 在本Provider boot时覆盖唯一session区段；更晚overlay仍按通常顺序生效。
    #[must_use]
    pub fn with_cookie_config(mut self, config: SessionConfig) -> Self {
        self.config = Some(config);
        self
    }
}
impl ServiceProvider for SessionServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        use crate::config::repository::SectionSchema as S;
        app.config_section(
            "session",
            SessionConfig::default(),
            S::object([
                ("driver", S::String.nullable()),
                ("cookie", S::String.nullable()),
                ("path", S::String),
                ("domain", S::String.nullable()),
                ("secure", S::Boolean.nullable()),
                ("http_only", S::Boolean),
                ("same_site", S::String.nullable()),
                ("lifetime", S::Integer),
                ("expire_on_close", S::Boolean),
                ("partitioned", S::Boolean),
                ("max_bytes", S::Integer),
            ]),
            |_: &SessionConfig| Ok(()),
        )?;
        let repository = app.config_repository();
        app.singleton::<SessionSettings, _>(move |_| {
            let repository = repository.clone();
            Box::pin(async move { Ok(Arc::new(SessionSettings::read(&repository)?)) })
        });
        let custom = self.store.clone();
        app.singleton::<SessionStoreHandle, _>(move |resolver| {
            let custom = custom.clone();
            Box::pin(async move {
                let settings = resolver.resolve::<SessionSettings>().await?;
                let store: Arc<dyn SessionStore> = if let Some(store) = custom {
                    store
                } else {
                    match settings.config.driver.as_deref() {
                        Some("database") => Arc::new(DatabaseSessionStore::new(
                            (*resolver.resolve::<crate::db::Db>().await?).clone(),
                        )),
                        Some("array") => Arc::new(MemorySessionStore::default()),
                        _ => {
                            return Err(Error::custom(
                                "session_config_invalid",
                                "Session driver要求公开with_store实现",
                            ));
                        }
                    }
                };
                Ok(Arc::new(SessionStoreHandle(store)))
            })
        });
        app.scoped::<SessionSlot, _>(|_| Box::pin(async { Ok(Arc::new(SessionSlot::default())) }));
        app.scoped::<Session, _>(|resolver| {
            Box::pin(async move {
                let slot = resolver.resolve::<SessionSlot>().await?;
                Ok(Arc::new(slot.0.get().cloned().ok_or_else(missing)?))
            })
        });
        let ready = Ready::default();
        app.instance(Arc::new(ready.clone()));
        app.singleton::<Installed, _>(move |resolver| {
            let ready = ready.clone();
            Box::pin(async move {
                let settings = resolver.resolve::<SessionSettings>().await?;
                let layer = if settings.config.driver.is_some() {
                    let store = resolver.resolve::<SessionStoreHandle>().await?;
                    Some(super::middleware::SessionLayer {
                        settings: (*settings).clone(),
                        store: store.0.clone(),
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
        });
        app.require::<Installed>();
        Ok(())
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
                        && settings
                            .config
                            .driver
                            .as_deref()
                            .is_some_and(|driver| !matches!(driver, "database" | "array"))
                    {
                        return Err(Error::Config(crate::config::ConfigError::Invalid(
                            "session.driver: 需要公开 with_store 实现".into(),
                        )));
                    }
                    Ok(())
                })
            });
            let ready = app.resolve::<Ready>().await?;
            app.http_layer_factory(move || {
                let layer = ready.0.get().ok_or_else(missing)?.clone();
                Ok(axum::middleware::from_fn_with_state(
                    layer,
                    super::middleware::handle,
                ))
            })?;
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
