use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock, RwLock},
    time::Duration,
};

use serde::Deserialize;

use crate::{
    cache::CacheManager,
    container::Container,
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
    ArraySessionHandler, CacheBasedSessionHandler, CookieSessionHandler, DatabaseSessionHandler,
    DatabaseSessionMetadata, EncryptedStore, FileSessionHandler, NullSessionHandler, SessionError,
    SessionResult, Store, Value,
};

#[derive(Deserialize)]
#[serde(default)]
struct Settings {
    driver: Option<String>,
    cookie: String,
    lifetime: u64,
    expire_on_close: bool,
    connection: Option<String>,
    store: Option<String>,
    prefix: Option<String>,
    files: Option<PathBuf>,
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
            store: None,
            prefix: None,
            files: None,
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
    config: Value,
    database: Arc<DatabaseManager>,
    array: ArraySessionHandler,
    cache: Arc<CacheManager>,
    cache_handler: OnceLock<CacheBasedSessionHandler>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
    base: PathBuf,
    paths: Option<Arc<crate::foundation::Paths>>,
    lifetime: Duration,
}

type DriverFactory =
    dyn Fn(&Container, &Request) -> SessionResult<Box<dyn SessionHandler>> + Send + Sync;

pub(super) struct RequestSession {
    pub(super) store: Store,
    pub(super) database_user: Option<Arc<OnceLock<crate::database::Value>>>,
}

impl SessionManager {
    pub(crate) fn new(
        config: Option<Value>,
        database: Arc<DatabaseManager>,
        cache: Arc<CacheManager>,
        base: PathBuf,
    ) -> SessionResult<Self> {
        let raw_config = config.clone().unwrap_or(Value::Null);
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
            config: raw_config,
            database,
            array: ArraySessionHandler::new(lifetime),
            cache,
            cache_handler: OnceLock::new(),
            factories: RwLock::default(),
            base,
            paths: None,
            lifetime,
        })
    }

    #[allow(non_snake_case)]
    pub fn shouldBlock(&self) -> bool {
        self.settings.block
    }
    #[allow(non_snake_case)]
    pub fn blockDriver(&self) -> Option<&str> {
        self.settings.block_store.as_deref()
    }
    #[allow(non_snake_case)]
    pub fn defaultRouteBlockLockSeconds(&self) -> u64 {
        self.settings.block_lock_seconds
    }
    #[allow(non_snake_case)]
    pub fn defaultRouteBlockWaitSeconds(&self) -> u64 {
        self.settings.block_wait_seconds
    }
    /// 返回装配时的配置快照，保留自定义配置字段。
    #[allow(non_snake_case)]
    pub fn getSessionConfig(&self) -> &Value {
        &self.config
    }

    pub(super) fn with_paths(mut self, paths: Arc<crate::foundation::Paths>) -> Self {
        self.paths = Some(paths);
        self
    }

    /// Registers a creator for a fresh handler on each request using the named driver.
    pub fn extend<H: SessionHandler + 'static>(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(&Container, &Request) -> SessionResult<H> + Send + Sync + 'static,
    ) -> SessionResult<&Self> {
        let driver = driver.into();
        if driver.trim().is_empty() {
            return Err(SessionError::Configuration);
        }
        let previous = self
            .factories
            .write()
            .map_err(|_| SessionError::Storage)?
            .insert(
                driver,
                Arc::new(move |container, request| Ok(Box::new(factory(container, request)?))),
            );
        drop(previous);
        Ok(self)
    }

    fn cache_handler(&self, driver: &str) -> SessionResult<CacheBasedSessionHandler> {
        if let Some(handler) = self.cache_handler.get() {
            return Ok(handler.clone());
        }
        let store = self
            .settings
            .store
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(driver);
        let repository = if driver == "redis" {
            self.cache
                .redis_session_store(
                    store,
                    self.settings.connection.as_deref(),
                    self.settings.prefix.as_deref(),
                )
                .map(Arc::new)
        } else {
            self.cache.store(Some(store))
        }
        .map_err(|_| SessionError::Configuration)?;
        Ok(self
            .cache_handler
            .get_or_init(|| CacheBasedSessionHandler::new(repository, self.lifetime))
            .clone())
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
        if !self.shouldBlock() && options.is_none_or(|route| route.lock_seconds == 0) {
            return Ok(None);
        }
        if self.settings.driver.as_deref() == Some("cookie") {
            return Err(SessionError::Configuration.into());
        }
        let lease = options
            .map(|route| route.lock_seconds)
            .filter(|seconds| *seconds != 0)
            .unwrap_or(self.defaultRouteBlockLockSeconds());
        let wait = options.map_or(self.defaultRouteBlockWaitSeconds(), |route| {
            route.wait_seconds
        });
        super::SessionLock::acquire(self.blockDriver(), id, lease, wait)
            .await
            .map(Some)
            .map_err(Into::into)
    }

    pub(super) fn build(
        &self,
        request: &Request,
        id: Option<&str>,
    ) -> SessionResult<Option<RequestSession>> {
        let mut database_user = None;
        let Some(driver) = self.settings.driver.as_deref() else {
            return Ok(None);
        };
        let custom = self
            .factories
            .read()
            .map_err(|_| SessionError::Storage)?
            .get(driver)
            .cloned();
        let handler: Box<dyn SessionHandler> = if let Some(factory) = custom {
            let container = facades::current_container().ok_or(SessionError::Configuration)?;
            factory(&container, request)?
        } else {
            match driver {
                "database" => {
                    let db = self
                        .database
                        .connection(self.settings.connection.as_deref())
                        .map_err(|_| SessionError::Storage)?;
                    let mut handler = DatabaseSessionHandler::new(
                        db,
                        &self.settings.table,
                        self.settings.lifetime,
                    )?;
                    handler.setMetadata(DatabaseSessionMetadata {
                        user_id: None,
                        ip_address: Some(request.ip().map(|address| address.to_string())),
                        user_agent: Some(
                            request
                                .headers()
                                .get(::http::header::USER_AGENT)
                                .map(|agent| String::from_utf8_lossy(agent.as_bytes()).into_owned())
                                .unwrap_or_default(),
                        ),
                    });
                    database_user = Some(handler.track_request_user());
                    Box::new(handler)
                }
                "array" => Box::new(self.array.clone()),
                "file" | "native" => {
                    let directory = match &self.settings.files {
                        Some(path) if path.as_os_str().is_empty() => {
                            return Err(SessionError::Configuration);
                        }
                        Some(path) => self.base.join(path),
                        None => self.paths.as_ref().map_or_else(
                            || self.base.join("storage/framework/sessions"),
                            |paths| paths.storage_path("framework/sessions"),
                        ),
                    };
                    Box::new(FileSessionHandler::new(directory, self.lifetime)?)
                }
                "null" => Box::new(NullSessionHandler),
                "cookie" => Box::new(CookieSessionHandler::for_request(
                    request,
                    id,
                    self.settings.lifetime,
                    self.settings.expire_on_close,
                )?),
                "redis" | "apc" | "memcached" | "dynamodb" => Box::new(self.cache_handler(driver)?),
                _ => return Err(SessionError::Configuration),
            }
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
        let store = Store::new(&self.settings.cookie, handler)?;
        Ok(Some(RequestSession {
            store,
            database_user,
        }))
    }

    pub(crate) async fn collect_garbage(&self, store: &mut Store) -> SessionResult<()> {
        let [hits, total] = self.settings.lottery;
        if hits > 0 && fastrand::u32(0..total) < hits {
            store.gc(self.lifetime).await?;
        }
        Ok(())
    }

    pub(crate) fn xsrf_cookie(
        &self,
        token: &str,
        stateful_frontend: bool,
    ) -> Result<Cookie, crate::http::HttpError> {
        let minutes =
            i64::try_from(self.settings.lifetime).map_err(|_| SessionError::Configuration)?;
        Ok(facades::Cookie::make(
            "XSRF-TOKEN",
            token,
            minutes,
            &CookieOptions {
                http_only: false,
                same_site: stateful_frontend.then_some(crate::cookie::SameSite::Lax),
                partitioned: self.settings.partitioned,
                ..CookieOptions::default()
            },
        )?)
    }

    pub(crate) fn cookie(
        &self,
        store: &Store,
        stateful_frontend: bool,
    ) -> Result<Cookie, crate::http::HttpError> {
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
                http_only: stateful_frontend || self.settings.http_only,
                same_site: stateful_frontend.then_some(crate::cookie::SameSite::Lax),
                partitioned: self.settings.partitioned,
                ..CookieOptions::default()
            },
        )?)
    }
}
