use std::{sync::Arc, time::Duration};

use serde::Deserialize;

use crate::{
    contracts::{
        container::Container as _,
        encryption::Encrypter,
        session::{Session, SessionHandler},
    },
    cookie::{Cookie, CookieOptions},
    database::DatabaseManager,
    http::Request,
    support::facades,
};

use super::{
    ArraySessionHandler, DatabaseSessionHandler, DatabaseSessionMetadata, EncryptedStore,
    SessionError, SessionResult, Store, Value,
};

#[derive(Deserialize)]
#[serde(default)]
struct Settings {
    driver: Option<String>,
    cookie: String,
    lifetime: u64,
    expire_on_close: bool,
    connection: Option<String>,
    table: String,
    http_only: bool,
    partitioned: bool,
    encrypt: bool,
    lottery: [u32; 2],
    block: bool,
    block_store: Option<String>,
    block_lock_seconds: u64,
    block_wait_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            driver: None,
            cookie: "openrust_session".into(),
            lifetime: 120,
            expire_on_close: false,
            connection: None,
            table: "sessions".into(),
            http_only: true,
            partitioned: false,
            encrypt: false,
            lottery: [2, 100],
            block: false,
            block_store: None,
            block_lock_seconds: 10,
            block_wait_seconds: 10,
        }
    }
}

/// Selects a session driver and creates a fresh store for each request.
pub struct SessionManager {
    settings: Settings,
    database: Arc<DatabaseManager>,
    array: ArraySessionHandler,
    lifetime: Duration,
}

impl SessionManager {
    pub(crate) fn new(
        config: Option<Value>,
        database: Arc<DatabaseManager>,
    ) -> SessionResult<Self> {
        let settings: Settings = match config {
            None | Some(Value::Null) => Settings::default(),
            Some(config) => {
                serde_json::from_value(config).map_err(|_| SessionError::Configuration)?
            }
        };
        let lifetime = settings
            .lifetime
            .checked_mul(60)
            .filter(|seconds| i64::try_from(*seconds).is_ok())
            .ok_or(SessionError::Configuration)?;
        if settings.lottery[1] == 0 || settings.lottery[0] > settings.lottery[1] {
            return Err(SessionError::Configuration);
        }
        let lifetime = Duration::from_secs(lifetime);
        Ok(Self {
            settings,
            database,
            array: ArraySessionHandler::new(lifetime),
            lifetime,
        })
    }

    pub(crate) fn cookie_name(&self) -> &str {
        &self.settings.cookie
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.settings.driver.is_some()
    }

    pub(crate) async fn lock(
        &self,
        options: Option<super::RouteBlock>,
        id: &str,
    ) -> Result<Option<super::SessionLock>, crate::http::HttpError> {
        if !self.settings.block && options.is_none_or(|route| route.lock_seconds == 0) {
            return Ok(None);
        }
        let lease = options
            .map(|route| route.lock_seconds)
            .filter(|seconds| *seconds != 0)
            .unwrap_or(self.settings.block_lock_seconds);
        let wait = options.map_or(self.settings.block_wait_seconds, |route| route.wait_seconds);
        super::SessionLock::acquire(self.settings.block_store.as_deref(), id, lease, wait)
            .await
            .map(Some)
            .map_err(super::blocking::http_error)
    }

    pub(crate) fn build(&self, request: &Request) -> SessionResult<Option<Store>> {
        let handler: Box<dyn SessionHandler> = match self.settings.driver.as_deref() {
            None => return Ok(None),
            Some("database") => {
                let db = self
                    .database
                    .connection(self.settings.connection.as_deref())
                    .map_err(|_| SessionError::Storage)?;
                let mut handler =
                    DatabaseSessionHandler::new(db, &self.settings.table, self.settings.lifetime)?;
                handler.setMetadata(DatabaseSessionMetadata {
                    user_id: None,
                    ip_address: Some(
                        request
                            .parts()
                            .extensions
                            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                            .map(|info| info.0.ip().to_string()),
                    ),
                    user_agent: Some(
                        request
                            .headers()
                            .get(::http::header::USER_AGENT)
                            .map(|agent| String::from_utf8_lossy(agent.as_bytes()).into_owned())
                            .unwrap_or_default(),
                    ),
                });
                Box::new(handler)
            }
            Some("array") => Box::new(self.array.clone()),
            Some(_) => return Err(SessionError::Configuration),
        };
        let handler = if self.settings.encrypt {
            let encrypter = facades::current_container()
                .ok_or(SessionError::Configuration)?
                .get::<dyn Encrypter>(crate::container::ServiceKey::typed::<dyn Encrypter>())
                .map_err(|_| SessionError::Configuration)?;
            Box::new(EncryptedStore::new(handler, encrypter))
        } else {
            handler
        };
        Store::new(&self.settings.cookie, handler).map(Some)
    }

    pub(crate) async fn collect_garbage(&self, store: &mut Store) -> SessionResult<()> {
        let [hits, total] = self.settings.lottery;
        if hits > 0 && fastrand::u32(0..total) < hits {
            store.gc(self.lifetime).await?;
        }
        Ok(())
    }

    pub(crate) fn xsrf_cookie(&self, token: &str) -> Result<Cookie, crate::http::HttpError> {
        let minutes =
            i64::try_from(self.settings.lifetime).map_err(|_| SessionError::Configuration)?;
        Ok(facades::Cookie::make(
            "XSRF-TOKEN",
            token,
            minutes,
            &CookieOptions {
                http_only: false,
                partitioned: self.settings.partitioned,
                ..CookieOptions::default()
            },
        )?)
    }

    pub(crate) fn cookie(&self, store: &Store) -> Result<Cookie, crate::http::HttpError> {
        let minutes = if self.settings.expire_on_close {
            0
        } else {
            i64::try_from(self.settings.lifetime).map_err(|_| SessionError::Configuration)?
        };
        Ok(facades::Cookie::make(
            store.getName(),
            store.getId(),
            minutes,
            &CookieOptions {
                http_only: self.settings.http_only,
                partitioned: self.settings.partitioned,
                ..CookieOptions::default()
            },
        )?)
    }
}
