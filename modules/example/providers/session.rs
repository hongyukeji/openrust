//! 调用方显式选择的请求绑定驱动示例；默认宿主不安装或覆盖已有 store。

use openrust::Result;
use openrust::foundation::RegisterContext;
use openrust::http::CookieOptions;
use openrust::session::{
    DatabaseSessionStore, GuardedDatabaseSessionStore, MemorySessionStore, SessionId,
    SessionRecord, SessionRequestContext, SessionStore, SessionStoreHandle, SessionWriteMode,
    SessionWriteOutcome,
};
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use serde_json::Value;
use std::sync::Arc;
use time::OffsetDateTime;

pub(crate) const PROBE_COOKIE: &str = "example_session_probe";
pub(crate) const REPLY_COOKIE: &str = "example_session_probe_reply";

/// 同一底层 driver 的请求实例；不保存第二套属性、不修改底层原子写语义。
pub struct ExampleSessionDriver {
    backend: Arc<dyn SessionStore>,
    request: Option<SessionRequestContext>,
}
impl ExampleSessionDriver {
    /// 显式装饰调用方选定的后端；不读取或替换容器里原有 factory。
    pub fn new(backend: Arc<dyn SessionStore>) -> Self {
        Self {
            backend,
            request: None,
        }
    }
    fn acknowledge(&self) -> Result<()> {
        let Some(context) = &self.request else {
            return Ok(());
        };
        let marker = match context.cookie(PROBE_COOKIE) {
            Some(Value::String(value)) if matches!(value.as_str(), "one" | "two") => value,
            _ => return Ok(()),
        };
        let jar = context.cookie_jar();
        jar.queue(
            jar.factory()
                .make(REPLY_COOKIE, marker, 0, CookieOptions::default())?,
        )
    }
}
impl SessionStore for ExampleSessionDriver {
    fn semantics(&self) -> openrust::session::SessionStoreSemantics {
        self.backend.semantics()
    }
    fn for_session(&self) -> Result<Option<Arc<dyn SessionStore>>> {
        let backend = self
            .backend
            .for_session()?
            .unwrap_or_else(|| self.backend.clone());
        Ok(Some(Arc::new(Self {
            backend,
            request: self.request.clone(),
        })))
    }
    fn activity_lifetime_seconds(&self) -> Option<i64> {
        self.backend.activity_lifetime_seconds()
    }
    fn set_database_user_id(&self, user_id: openrust::database::sea_orm::Value) -> Result<bool> {
        self.backend.set_database_user_id(user_id)
    }
    fn reset_exists(&self) -> Result<()> {
        self.backend.reset_exists()
    }
    fn accepts_request(&self) -> bool {
        // 装饰器自身也需要请求Cookie；底层可选metadata不得因needs_request=false丢失。
        self.needs_request() || self.backend.accepts_request()
    }
    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let deleted = self.backend.gc(lifetime_seconds).await?;
            if let Some(context) = &self.request {
                let jar = context.cookie_jar();
                jar.queue(jar.factory().make(
                    "example_session_gc",
                    format!("{lifetime_seconds}:{deleted}"),
                    0,
                    CookieOptions::default(),
                )?)?;
            }
            Ok(deleted)
        })
    }

    fn supports_session_authentication(&self, cookie_name: &str) -> bool {
        self.backend.supports_session_authentication(cookie_name)
    }
    fn guarantees(&self) -> openrust::session::SessionStoreGuarantees {
        self.backend.guarantees()
    }
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        let backend = if self.backend.accepts_request() {
            self.backend.bind_request(context.clone())?.ok_or_else(|| {
                openrust::Error::custom("example_session_binding_invalid", "示例底层驱动未绑定请求")
            })?
        } else {
            self.backend
                .for_session()?
                .unwrap_or_else(|| self.backend.clone())
        };
        Ok(Some(Arc::new(Self {
            backend,
            request: Some(context),
        })))
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let result = self.backend.load(id, max, expiry).await?;
            self.acknowledge()?;
            Ok(result)
        })
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let result = self.backend.write(record, mode, max).await?;
            self.acknowledge()?;
            Ok(result)
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.backend.delete(id)
    }
}

/// 只供调用方主动追加到已知测试/应用清单，绝不由默认模块 Provider 自动登记。
pub struct ExampleSessionDriverProvider {
    database: bool,
}
impl ExampleSessionDriverProvider {
    /// 显式 memory fixture，不宣称数据库故障回退。
    pub fn memory() -> Self {
        Self { database: false }
    }
    /// 沿当前应用同一个公开 Db 连接构造数据库 driver。
    pub fn database() -> Self {
        Self { database: true }
    }
}
impl ServiceProvider for ExampleSessionDriverProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let database = self.database;
            app.singleton::<SessionStoreHandle, _>(move |resolver| {
                Box::pin(async move {
                    let backend: Arc<dyn SessionStore> = if database {
                        let config =
                            resolver.section::<openrust::config::SessionConfig>("session")?;
                        let db = if let Some(connection) = &config.connection {
                            resolver
                                .resolve::<openrust::database::Databases>()
                                .await?
                                .get(connection)?
                        } else {
                            (*resolver.resolve::<openrust::database::Db>().await?).clone()
                        };
                        if config.driver.as_deref() == Some("database-guarded") {
                            Arc::new(GuardedDatabaseSessionStore::new(db).for_table(config.table)?)
                        } else {
                            Arc::new(
                                DatabaseSessionStore::new(db)
                                    .configured(config.table, config.lifetime)?,
                            )
                        }
                    } else {
                        Arc::new(MemorySessionStore::default())
                    };
                    Ok(Arc::new(SessionStoreHandle::new(
                        ExampleSessionDriver::new(backend),
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }
}

/// 显式 Cookie fixture 配置；调用方安装到独立 Application，不覆盖默认宿主。
pub fn cookie_session_provider() -> openrust::session::SessionServiceProvider {
    openrust::session::SessionServiceProvider::new().with_cookie_config(
        openrust::config::SessionConfig {
            driver: Some("cookie".into()),
            cookie: Some("example_module_session".into()),
            secure: Some(false),
            ..Default::default()
        },
    )
}
