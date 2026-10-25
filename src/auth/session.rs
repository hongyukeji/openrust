//! Shared database-backed HTTP sessions.

use std::sync::Arc;

use crate::config::ConfigError;
use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};
use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tower_sessions::cookie::SameSite;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};
use tower_sessions::{Expiry, SessionManagerLayer};

use crate::http::AppBuilderHttpExt;

/// Settings for the shared Cookie Session middleware.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Cookie name. Defaults to `openrust_session`.
    pub cookie_name: String,
    /// Whether browsers send the cookie only over HTTPS. Defaults to `true`.
    pub secure: bool,
    /// Whether the cookie is inaccessible to JavaScript (`HttpOnly`). Defaults to `true`.
    pub http_only: bool,
    /// `SameSite` policy: `strict`, `lax`, or `none`. Defaults to `lax`.
    pub same_site: String,
    /// Inactivity lifetime, in seconds. Defaults to two weeks.
    pub ttl_secs: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_name: "openrust_session".to_owned(),
            secure: true,
            http_only: true,
            same_site: "lax".to_owned(),
            ttl_secs: 1_209_600,
        }
    }
}

impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionConfig")
            .field("cookie_name", &self.cookie_name)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("ttl_secs", &self.ttl_secs)
            .finish()
    }
}

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

/// A PostgreSQL/MySQL Session store backed by the application's shared [`Db`].
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
    /// Creates a store over an existing framework database handle.
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

/// Registers the shared database Session store and global Cookie middleware.
pub struct SessionPlugin {
    config: Option<SessionConfig>,
    storage: SessionStorage,
}

impl SessionPlugin {
    /// Creates a Session plugin with secure default cookie settings.
    pub fn new() -> Self {
        Self {
            config: None,
            storage: SessionStorage::Database,
        }
    }

    /// Sets the Session cookie and expiry configuration.
    pub fn with_config(config: SessionConfig) -> Self {
        Self {
            config: Some(config),
            storage: SessionStorage::Database,
        }
    }

    /// Creates a Session plugin using an application supplied [`SessionStore`].
    pub fn with_store(store: impl SessionStore) -> Self {
        Self {
            config: None,
            storage: SessionStorage::Custom(Arc::new(store)),
        }
    }

    /// Overrides the cookie and expiry configuration for this plugin instance.
    #[must_use]
    pub fn with_cookie_config(mut self, config: SessionConfig) -> Self {
        self.config = Some(config);
        self
    }
}

impl Default for SessionPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for SessionPlugin {
    fn name(&self) -> &'static str {
        "session"
    }

    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<SessionService>().is_some() {
            return Err(Error::custom(
                "session_service_already_registered",
                "register only one Session plugin per application",
            ));
        }
        let config = match self.config.clone() {
            Some(config) => config,
            None => match app.section::<SessionConfig>("session") {
                Ok(config) => config,
                Err(ConfigError::Source(error)) if error.missing() => SessionConfig::default(),
                Err(error) => return Err(error.into()),
            },
        };
        let same_site = config.validate()?;
        let store: Arc<dyn SessionStore> = match &self.storage {
            SessionStorage::Database => {
                let db = app.get::<Db>().ok_or_else(|| {
                    Error::Config(ConfigError::Invalid(
                        "SessionPlugin requires DatabasePlugin to be registered first".into(),
                    ))
                })?;
                Arc::new(DatabaseSessionStore::new((*db).clone()))
            }
            SessionStorage::Custom(store) => store.clone(),
        };
        app.insert(SessionService);
        app.http_layer(
            SessionManagerLayer::new(SharedSessionStore(store))
                .with_name(config.cookie_name)
                .with_secure(config.secure)
                .with_http_only(config.http_only)
                .with_same_site(same_site)
                .with_expiry(Expiry::OnInactivity(time::Duration::seconds(
                    i64::try_from(config.ttl_secs).map_err(|_| {
                        invalid_session_config("ttl_secs exceeds supported duration")
                    })?,
                ))),
        );
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.state().try_get::<SessionService>()?;
            if matches!(&self.storage, SessionStorage::Database) {
                let db = app.state().try_get::<Db>()?;
                match db.backend()? {
                    DbBackend::Postgres | DbBackend::MySql => Ok(()),
                    _ => Err(Error::Config(ConfigError::Invalid(
                        "the Session store supports PostgreSQL and MySQL only".into(),
                    ))),
                }
            } else {
                Ok(())
            }
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

pub(super) struct SessionService;

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

fn invalid_session_config(detail: &str) -> Error {
    Error::Config(ConfigError::Invalid(format!("session: {detail}")))
}
