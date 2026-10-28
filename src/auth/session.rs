//! 基于共享数据库的 HTTP Session。

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::config::ConfigError;
use crate::config::SessionConfig;
use crate::core::{BoxFuture, Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tower_sessions::cookie::SameSite;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};
use tower_sessions::{Expiry, SessionManagerLayer};

#[cfg(feature = "session-redis")]
use redis::AsyncCommands as _;

impl SessionConfig {
    fn validate(&self) -> Result<SameSite> {
        if self.cookie_name.trim().is_empty()
            || self
                .cookie_name
                .bytes()
                .any(|byte| byte <= b' ' || byte == b';')
        {
            return Err(invalid_session_config("cookie_name must be a valid token"));
        }
        if self.ttl_secs == 0 || self.ttl_secs > i64::MAX as u64 {
            return Err(invalid_session_config(
                "ttl_secs must be positive and bounded",
            ));
        }
        match self.same_site.to_ascii_lowercase().as_str() {
            "strict" => Ok(SameSite::Strict),
            "lax" => Ok(SameSite::Lax),
            "none" if self.secure => Ok(SameSite::None),
            "none" => Err(invalid_session_config("SameSite=None requires secure=true")),
            _ => Err(invalid_session_config(
                "same_site must be strict, lax, or none",
            )),
        }
    }
}

/// 使用应用共享 [`Db`] 的 PostgreSQL/MySQL Session store。
#[derive(Clone)]
pub struct DatabaseSessionStore {
    db: Db,
}

impl std::fmt::Debug for DatabaseSessionStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseSessionStore")
    }
}

impl DatabaseSessionStore {
    /// 基于现有框架数据库句柄创建 store。
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    async fn create_record(&self, record: &Record) -> session_store::Result<()> {
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "INSERT INTO tb_sessions (id_hash, data, expires_at, created_at) VALUES ($1, $2, $3, CURRENT_TIMESTAMP)"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_sessions (id_hash, data, expires_at, created_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP)"
            }
            _ => return Err(store_backend_error()),
        };
        let values = [
            id_hash(&record.id).into(),
            serde_json::to_value(&record.data)
                .map_err(|_| session_store::Error::Encode("session data encoding failed".into()))?
                .into(),
            to_chrono(record.expiry_date)
                .ok_or_else(|| session_store::Error::Encode("invalid session expiry".into()))?
                .into(),
        ];
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql).map_err(|_| store_backend_error())?,
                values,
            ))
            .await
            .map_err(|_| store_backend_error())?;
        Ok(())
    }

    fn connection(&self) -> session_store::Result<DatabaseConnection> {
        let connection = self.db.connection().map_err(|_| store_backend_error())?;
        match connection.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql => Ok(connection),
            _ => Err(store_backend_error()),
        }
    }
}

/// 使用 Redis 共享存储的 Session store。
#[cfg(feature = "session-redis")]
#[derive(Clone)]
pub struct RedisSessionStore {
    connection: redis::aio::ConnectionManager,
    namespace: String,
}

#[cfg(feature = "session-redis")]
impl std::fmt::Debug for RedisSessionStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RedisSessionStore")
            .field("namespace", &self.namespace)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "session-redis")]
impl RedisSessionStore {
    /// 连接 Redis 并建立一个隔离的 key namespace。
    ///
    /// URL 不会被保存在 store 或写入错误信息。`namespace` 仅允许 1–63 个 ASCII
    /// 字母、数字、`_` 或 `-`，用于隔离共享 Redis 中不同应用的 Session。
    pub async fn connect(dsn: impl Into<String>, namespace: impl Into<String>) -> Result<Self> {
        let namespace = namespace.into();
        if namespace.is_empty()
            || namespace.len() > 63
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(invalid_session_config(
                "Redis namespace must contain 1 to 63 ASCII letters, digits, underscores, or hyphens",
            ));
        }
        let client = redis::Client::open(dsn.into()).map_err(|_| {
            Error::Config(ConfigError::Invalid(
                "session Redis URL is invalid".to_owned(),
            ))
        })?;
        let connection = client.get_connection_manager().await.map_err(|_| {
            Error::custom(
                "session_redis_unavailable",
                "Redis Session store is unavailable",
            )
        })?;
        Ok(Self {
            connection,
            namespace,
        })
    }

    fn key(&self, id: &Id) -> String {
        format!("openrust:session:{}:{}", self.namespace, id_hash(id))
    }
}

#[cfg(feature = "session-redis")]
#[async_trait::async_trait]
impl SessionStore for RedisSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let ttl = redis_ttl_millis(record.expiry_date)?;
        let value = serde_json::to_vec(record)
            .map_err(|_| session_store::Error::Encode("session data encoding failed".into()))?;
        let mut connection = self.connection.clone();
        let created: Option<String> = redis::cmd("SET")
            .arg(self.key(&record.id))
            .arg(value)
            .arg("PX")
            .arg(ttl)
            .arg("NX")
            .query_async(&mut connection)
            .await
            .map_err(|_| redis_session_backend_error())?;
        if created.is_none() {
            return Err(redis_session_backend_error());
        }
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let ttl = redis_ttl_millis(record.expiry_date)?;
        let value = serde_json::to_vec(record)
            .map_err(|_| session_store::Error::Encode("session data encoding failed".into()))?;
        let mut connection = self.connection.clone();
        let _: Option<String> = redis::cmd("SET")
            .arg(self.key(&record.id))
            .arg(value)
            .arg("PX")
            .arg(ttl)
            .arg("XX")
            .query_async(&mut connection)
            .await
            .map_err(|_| redis_session_backend_error())?;
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let mut connection = self.connection.clone();
        let value: Option<Vec<u8>> = connection
            .get(self.key(session_id))
            .await
            .map_err(|_| redis_session_backend_error())?;
        let Some(value) = value else {
            return Ok(None);
        };
        let record: Record = serde_json::from_slice(&value)
            .map_err(|_| session_store::Error::Decode("invalid stored session data".into()))?;
        if record.id != *session_id {
            return Err(session_store::Error::Decode(
                "stored session identifier does not match its key".into(),
            ));
        }
        if record.expiry_date <= time::OffsetDateTime::now_utc() {
            return Ok(None);
        }
        Ok(Some(record))
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let mut connection = self.connection.clone();
        let _: i64 = connection
            .del(self.key(session_id))
            .await
            .map_err(|_| redis_session_backend_error())?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.create_record(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "UPDATE tb_sessions SET data = $1, expires_at = $2 WHERE id_hash = $3 AND expires_at > CURRENT_TIMESTAMP"
            }
            DbBackend::MySql => {
                "UPDATE tb_sessions SET data = ?, expires_at = ? WHERE id_hash = ? AND expires_at > CURRENT_TIMESTAMP"
            }
            _ => return Err(store_backend_error()),
        };
        let values = [
            serde_json::to_value(&record.data)
                .map_err(|_| session_store::Error::Encode("session data encoding failed".into()))?
                .into(),
            to_chrono(record.expiry_date)
                .ok_or_else(|| session_store::Error::Encode("invalid session expiry".into()))?
                .into(),
            id_hash(&record.id).into(),
        ];
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql).map_err(|_| store_backend_error())?,
                values,
            ))
            .await
            .map_err(|_| store_backend_error())?;
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT data, expires_at FROM tb_sessions WHERE id_hash = $1 AND expires_at > CURRENT_TIMESTAMP"
            }
            DbBackend::MySql => {
                "SELECT data, expires_at FROM tb_sessions WHERE id_hash = ? AND expires_at > CURRENT_TIMESTAMP"
            }
            _ => return Err(store_backend_error()),
        };
        let row = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql).map_err(|_| store_backend_error())?,
                [id_hash(session_id).into()],
            ))
            .await
            .map_err(|_| store_backend_error())?;
        let Some(row) = row else {
            return Ok(None);
        };
        let data = row
            .try_get::<serde_json::Value>("", "data")
            .map_err(|_| session_store::Error::Decode("invalid stored session data".into()))?;
        let data = serde_json::from_value(data)
            .map_err(|_| session_store::Error::Decode("invalid stored session data".into()))?;
        let expiry = row
            .try_get::<DateTime<Utc>>("", "expires_at")
            .map_err(|_| session_store::Error::Decode("invalid stored session expiry".into()))?;
        let expiry_date = time::OffsetDateTime::from_unix_timestamp_nanos(
            i128::from(expiry.timestamp()) * 1_000_000_000
                + i128::from(expiry.timestamp_subsec_nanos()),
        )
        .map_err(|_| session_store::Error::Decode("invalid stored session expiry".into()))?;
        Ok(Some(Record {
            id: *session_id,
            data,
            expiry_date,
        }))
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => "DELETE FROM tb_sessions WHERE id_hash = $1",
            DbBackend::MySql => "DELETE FROM tb_sessions WHERE id_hash = ?",
            _ => return Err(store_backend_error()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql).map_err(|_| store_backend_error())?,
                [id_hash(session_id).into()],
            ))
            .await
            .map_err(|_| store_backend_error())?;
        Ok(())
    }
}

/// 允许框架认证流程精确撤销一个共享 Session 的窄句柄。
#[derive(Clone)]
pub struct SessionStoreHandle(Arc<dyn SessionStore>);

impl std::fmt::Debug for SessionStoreHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionStoreHandle([REDACTED])")
    }
}

impl SessionStoreHandle {
    /// 从应用选定的 store 创建受限撤销句柄。
    pub fn new(store: impl SessionStore + 'static) -> Self {
        Self(Arc::new(store))
    }

    /// 删除指定的不透明 Session ID；错误时调用方必须按失败处理。
    pub async fn delete(&self, session_id: &str) -> Result<()> {
        let id = session_id.parse::<Id>().map_err(|_| Error::Unauthorized)?;
        self.0.delete(&id).await.map_err(|_| {
            Error::custom_with_status(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "session_store_unavailable",
                "shared Session storage is unavailable",
            )
        })
    }

    /// 读取已经认证的 Session 在共享 store 中持久保存的截止时间，不续期。
    ///
    /// 调用方须先通过 `AuthLayer` 验证账户；此处再比较请求快照和持久记录的账户 ID，
    /// 在记录不存在、已过期或 store 不可用时拒绝，不给长连接延长有效期。
    pub async fn authorized_until(
        &self,
        session: &tower_sessions::Session,
        user_id: i64,
    ) -> Result<SystemTime> {
        if session
            .get::<i64>(crate::auth::database::AUTH_USER_ID_KEY)
            .await
            .map_err(|_| Error::Unauthorized)?
            != Some(user_id)
        {
            return Err(Error::Unauthorized);
        }
        let id = session.id().ok_or(Error::Unauthorized)?;
        let record = self
            .0
            .load(&id)
            .await
            .map_err(|_| {
                Error::custom_with_status(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "session_store_unavailable",
                    "shared Session storage is unavailable",
                )
            })?
            .ok_or(Error::Unauthorized)?;
        if record
            .data
            .get(crate::auth::database::AUTH_USER_ID_KEY)
            .and_then(serde_json::Value::as_i64)
            != Some(user_id)
        {
            return Err(Error::Unauthorized);
        }
        if record.expiry_date <= time::OffsetDateTime::now_utc() {
            return Err(Error::Unauthorized);
        }
        let seconds =
            u64::try_from(record.expiry_date.unix_timestamp()).map_err(|_| Error::Unauthorized)?;
        let nanos = record.expiry_date.nanosecond();
        UNIX_EPOCH
            .checked_add(Duration::new(seconds, nanos))
            .ok_or(Error::Unauthorized)
    }
}

/// 在统一容器中装配共享 Session store 和 Cookie 中间件。
pub struct SessionServiceProvider {
    config: Option<SessionConfig>,
    storage: SessionStorage,
}

#[derive(Clone)]
pub(crate) struct SessionSettings(pub(crate) SessionConfig);

impl SessionServiceProvider {
    /// 使用数据库 store 和配置仓储中的 Cookie 设置。
    pub fn new() -> Self {
        Self {
            config: None,
            storage: SessionStorage::Database,
        }
    }
    /// 使用调用方提供的共享 store。
    pub fn with_store(store: impl SessionStore + 'static) -> Self {
        Self {
            config: None,
            storage: SessionStorage::Custom(Arc::new(store)),
        }
    }
    /// 使用显式 Cookie 配置。
    #[must_use]
    pub fn with_cookie_config(mut self, config: SessionConfig) -> Self {
        self.config = Some(config);
        self
    }
}

impl Default for SessionServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::core::ServiceProvider for SessionServiceProvider {
    fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
        use crate::config::repository::SectionSchema as S;
        app.config_section(
            "session",
            SessionConfig::default(),
            S::object([
                ("cookie_name", S::String),
                ("secure", S::Boolean),
                ("http_only", S::Boolean),
                ("same_site", S::String),
                ("ttl_secs", S::Integer),
            ]),
            |config: &SessionConfig| config.validate().map(|_| ()),
        )?;
        let explicit_config = self.config.clone();
        app.singleton::<SessionSettings, _>(move |resolver| {
            let explicit_config = explicit_config.clone();
            Box::pin(async move {
                let config = match explicit_config {
                    Some(config) => config,
                    None => resolver.section::<SessionConfig>("session")?,
                };
                config.validate()?;
                Ok(Arc::new(SessionSettings(config)))
            })
        });
        let custom = match &self.storage {
            SessionStorage::Custom(store) => Some(Arc::clone(store)),
            SessionStorage::Database => None,
        };
        app.singleton::<SessionStoreHandle, _>(move |resolver| {
            let custom = custom.clone();
            Box::pin(async move {
                let store: Arc<dyn SessionStore> = if let Some(store) = custom {
                    store
                } else {
                    let db = resolver.resolve::<Db>().await?;
                    match db.backend()? {
                        DbBackend::Postgres | DbBackend::MySql => {}
                        _ => {
                            return Err(Error::Config(ConfigError::Invalid(
                                "the Session store supports PostgreSQL and MySQL only".into(),
                            )));
                        }
                    }
                    Arc::new(DatabaseSessionStore::new((*db).clone()))
                };
                Ok(Arc::new(SessionStoreHandle(store)))
            })
        });
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.resolve::<SessionSettings>().await?.0.clone();
            let same_site = config.validate()?;
            let store = app.resolve::<SessionStoreHandle>().await?;
            app.http_layer(
                SessionManagerLayer::new(SharedSessionStore(Arc::clone(&store.0)))
                    .with_name(config.cookie_name)
                    .with_secure(config.secure)
                    .with_http_only(config.http_only)
                    .with_same_site(same_site)
                    .with_expiry(Expiry::OnInactivity(time::Duration::seconds(
                        i64::try_from(config.ttl_secs).map_err(|_| {
                            invalid_session_config("ttl_secs exceeds supported duration")
                        })?,
                    ))),
            )?;
            Ok(())
        })
    }
}

enum SessionStorage {
    Database,
    Custom(Arc<dyn SessionStore>),
}

#[derive(Debug, Clone)]
struct SharedSessionStore(Arc<dyn SessionStore>);

#[async_trait::async_trait]
impl SessionStore for SharedSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.0.create(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        self.0.save(record).await
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        self.0.load(session_id).await
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.0.delete(session_id).await
    }
}

fn id_hash(id: &Id) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(id.0.to_le_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn to_chrono(value: time::OffsetDateTime) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value.unix_timestamp(), value.nanosecond())
}

fn store_backend_error() -> session_store::Error {
    session_store::Error::Backend("database Session store operation failed".into())
}

#[cfg(feature = "session-redis")]
fn redis_session_backend_error() -> session_store::Error {
    session_store::Error::Backend("Redis Session store operation failed".into())
}

#[cfg(feature = "session-redis")]
fn redis_ttl_millis(expiry: time::OffsetDateTime) -> session_store::Result<u64> {
    let remaining_nanos =
        expiry.unix_timestamp_nanos() - time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    if remaining_nanos <= 0 {
        return Err(session_store::Error::Encode(
            "session expiry must be in the future".into(),
        ));
    }
    let millis = (remaining_nanos + 999_999) / 1_000_000;
    u64::try_from(millis)
        .map_err(|_| session_store::Error::Encode("session expiry exceeds Redis limits".into()))
}

fn invalid_session_config(detail: &str) -> Error {
    Error::Config(ConfigError::Invalid(format!("session: {detail}")))
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::{BootContext, ProviderBuilder, RegisterContext, ServiceProvider};
    use figment::Figment;
    use tower_sessions::MemoryStore;

    struct InvalidOverlay;
    impl ServiceProvider for InvalidOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("session", serde_json::json!({"ttl_secs": 0})) })
        }
    }

    struct CleanupProbe(Arc<std::sync::atomic::AtomicBool>);
    impl ServiceProvider for CleanupProbe {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            let closed = Arc::clone(&self.0);
            Box::pin(async move {
                app.resources().on_shutdown(move || {
                    Box::pin(async move {
                        closed.store(true, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                Ok(())
            })
        }
    }

    struct OverrideStore;
    impl ServiceProvider for OverrideStore {
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            app.instance(Arc::new(SessionStoreHandle::new(MemoryStore::default())));
            Ok(())
        }
    }

    #[tokio::test]
    async fn custom_store_and_later_override_do_not_require_database() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(SessionServiceProvider::new());
        builder.provider(OverrideStore);
        let runtime = builder.start(Figment::new()).await?;
        runtime.container().resolve::<SessionStoreHandle>().await?;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn explicit_cookie_name_is_the_same_snapshot_used_by_authentication() -> Result<()> {
        let config = SessionConfig {
            cookie_name: "custom_session".into(),
            ..SessionConfig::default()
        };
        let mut builder = ProviderBuilder::new();
        builder.provider(
            SessionServiceProvider::with_store(MemoryStore::default()).with_cookie_config(config),
        );
        let runtime = builder.start(Figment::new()).await?;
        let settings = runtime.container().resolve::<SessionSettings>().await?;
        assert_eq!(settings.0.cookie_name, "custom_session");
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn missing_database_and_invalid_late_overlay_fail_closed() {
        let mut missing = ProviderBuilder::new();
        missing.provider(SessionServiceProvider::new());
        assert!(missing.start(Figment::new()).await.is_err());
        let mut invalid = ProviderBuilder::new();
        invalid.provider(SessionServiceProvider::with_store(MemoryStore::default()));
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        invalid.provider(CleanupProbe(Arc::clone(&closed)));
        invalid.provider(InvalidOverlay);
        assert!(invalid.start(Figment::new()).await.is_err());
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
    }
}
