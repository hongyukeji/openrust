use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ::redis::{
    Client, ConnectionAddr, ConnectionInfo, IntoConnectionInfo, ProtocolVersion,
    aio::ConnectionManagerConfig, cluster::ClusterClient,
};
use serde::Deserialize;

use crate::contracts::{config::Repository, redis::Factory};

use super::{RedisError, RedisResult, connections::Connection};

type Connector = dyn Fn(&serde_json::Value, &str) -> RedisResult<Connection> + Send + Sync;

/// Resolves named Redis connections and manages their lifecycle.
pub struct RedisManager {
    config: Arc<dyn Repository>,
    connections: Mutex<HashMap<String, Arc<Connection>>>,
    pub(crate) events: Arc<CommandEvents>,
    custom_creators: RwLock<HashMap<String, Arc<Connector>>>,
}

#[derive(Default)]
pub(crate) struct CommandEvents {
    pub(crate) dispatcher: RwLock<Option<Arc<crate::events::Dispatcher>>>,
    pub(crate) enabled: AtomicBool,
}

#[derive(Deserialize)]
#[serde(default)]
struct Settings {
    url: Option<String>,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    database: i64,
    max_retries: usize,
    backoff_base: u64,
    backoff_cap: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            url: None,
            host: "127.0.0.1".into(),
            port: 6379,
            username: None,
            password: None,
            database: 0,
            max_retries: 3,
            backoff_base: 100,
            backoff_cap: 1000,
        }
    }
}

impl RedisManager {
    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            connections: Mutex::default(),
            events: Arc::default(),
            custom_creators: RwLock::default(),
        }
    }

    pub fn set_event_dispatcher(&self, dispatcher: Arc<crate::events::Dispatcher>) {
        *self
            .events
            .dispatcher
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(dispatcher);
    }

    pub fn enable_events(&self) {
        self.events.enabled.store(true, Ordering::Release);
    }

    pub fn disable_events(&self) {
        self.events.enabled.store(false, Ordering::Release);
    }

    /// 清除后续解析的缓存；已交给消费者的连接仍有效。
    pub fn purge(&self, name: Option<&str>) {
        self.connections
            .lock()
            .unwrap()
            .remove(name.filter(|name| !name.is_empty()).unwrap_or("default"));
    }

    pub fn extend(
        &self,
        driver: impl Into<String>,
        callback: impl Fn(&serde_json::Value, &str) -> RedisResult<Connection> + Send + Sync + 'static,
    ) -> &Self {
        self.custom_creators
            .write()
            .unwrap()
            .insert(driver.into(), Arc::new(callback));
        self
    }

    pub fn connection(&self, name: Option<&str>) -> RedisResult<Arc<Connection>> {
        let name = name.unwrap_or("default");
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(connection) = connections.get(name) {
            return Ok(connection.clone());
        }
        let config = self
            .config
            .get("database.redis")
            .ok_or_else(|| RedisError::UnknownConnection { name: name.into() })?;
        let prefix = match config.pointer("/options/prefix") {
            None | Some(serde_json::Value::Null) => String::new(),
            Some(serde_json::Value::String(prefix)) => prefix.clone(),
            _ => return Err(RedisError::InvalidConfiguration),
        };
        let direct = config
            .get(name)
            .filter(|value| !value.is_null() && !matches!(name, "options" | "clusters" | "client"));
        let driver = config
            .get("client")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("redis");
        let creator = self.custom_creators.read().unwrap().get(driver).cloned();
        let connection = if let Some(creator) = creator {
            creator(
                direct.ok_or_else(|| RedisError::UnknownConnection { name: name.into() })?,
                &prefix,
            )?
        } else if driver != "redis" {
            return Err(RedisError::InvalidConfiguration);
        } else if let Some(value) = direct {
            let settings: Settings = serde_json::from_value(value.clone())
                .map_err(|_| RedisError::InvalidConfiguration)?;
            settings.connect(prefix)?
        } else if let Some(seeds) = config
            .get("clusters")
            .and_then(|clusters| clusters.get(name))
            .filter(|_| name != "options")
        {
            cluster_connection(&config, seeds, prefix)?
        } else {
            return Err(RedisError::UnknownConnection { name: name.into() });
        };
        let connection = Arc::new(connection.with_events(name, self.events.clone()));
        connections.insert(name.into(), connection.clone());
        Ok(connection)
    }
}

impl Factory for RedisManager {
    fn connection(
        &self,
        name: Option<&str>,
    ) -> RedisResult<Arc<dyn crate::contracts::redis::Connection>> {
        Ok(self.connection(name)?)
    }
}

impl Settings {
    fn connect(self, prefix: String) -> RedisResult<Connection> {
        let config = ConnectionManagerConfig::new()
            .set_number_of_retries(self.max_retries)
            .set_min_delay(Duration::from_millis(self.backoff_base))
            .set_max_delay(Duration::from_millis(self.backoff_cap))
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(Some(Duration::from_secs(5)));
        let client =
            Client::open(self.connection_info()?).map_err(|_| RedisError::InvalidConfiguration)?;
        Ok(Connection::new(client, config, prefix))
    }

    fn connection_info(self) -> RedisResult<ConnectionInfo> {
        if self.database < 0 || self.backoff_base > self.backoff_cap {
            return Err(RedisError::InvalidConfiguration);
        }
        let info = if let Some(url) = self.url.filter(|url| !url.is_empty()) {
            let parsed = url::Url::parse(&url).map_err(|_| RedisError::InvalidConfiguration)?;
            if !matches!(parsed.scheme(), "redis" | "rediss") {
                return Err(RedisError::InvalidConfiguration);
            }
            let info = url
                .into_connection_info()
                .map_err(|_| RedisError::InvalidConfiguration)?;
            let mut settings = info.redis_settings().clone();
            if parsed.path().trim_matches('/').is_empty() {
                settings = settings.set_db(self.database);
            }
            if parsed.username().is_empty()
                && let Some(username) = self.username
            {
                settings = settings.set_username(username);
            }
            if parsed.password().is_none()
                && let Some(password) = self.password
            {
                settings = settings.set_password(password);
            }
            info.set_redis_settings(settings)
        } else {
            if self.host.is_empty() || self.port == 0 {
                return Err(RedisError::InvalidConfiguration);
            }
            let mut settings = ::redis::RedisConnectionInfo::default().set_db(self.database);
            if let Some(username) = self.username {
                settings = settings.set_username(username);
            }
            if let Some(password) = self.password {
                settings = settings.set_password(password);
            }
            "redis://127.0.0.1/"
                .into_connection_info()
                .expect("valid Redis address")
                .set_addr(ConnectionAddr::Tcp(self.host, self.port))
                .set_redis_settings(settings)
        };
        if matches!(info.addr(), ConnectionAddr::TcpTls { .. })
            && rustls::crypto::CryptoProvider::get_default().is_none()
        {
            // Rustls cannot infer a provider when workspace dependencies enable both backends.
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        Ok(info)
    }
}

fn cluster_connection(
    config: &serde_json::Value,
    seeds: &serde_json::Value,
    prefix: String,
) -> RedisResult<Connection> {
    if config
        .pointer("/options/cluster")
        .is_some_and(|mode| mode.as_str() != Some("redis"))
    {
        return Err(RedisError::InvalidConfiguration);
    }
    let settings: Vec<Settings> =
        serde_json::from_value(seeds.clone()).map_err(|_| RedisError::InvalidConfiguration)?;
    let first = settings.first().ok_or(RedisError::InvalidConfiguration)?;
    const EXPONENT_BASE: u64 = 2;
    const EXPONENT_FACTOR: u64 = 10;
    let retries = u32::try_from(first.max_retries).map_err(|_| RedisError::InvalidConfiguration)?;
    let min_wait = first.backoff_base;
    let max_wait = first.backoff_cap;
    // The native client computes its exponential delay before clamping it.
    if EXPONENT_BASE
        .checked_pow(retries.saturating_add(1))
        .and_then(|exponent| exponent.checked_mul(EXPONENT_FACTOR))
        .is_none()
        || min_wait == u64::MAX
    {
        return Err(RedisError::InvalidConfiguration);
    }
    let seeds = settings
        .into_iter()
        .map(|settings| {
            let info = settings.connection_info()?;
            if info.redis_settings().db() != 0 {
                return Err(RedisError::InvalidConfiguration);
            }
            let redis = info
                .redis_settings()
                .clone()
                .set_protocol(ProtocolVersion::RESP3);
            Ok(info.set_redis_settings(redis))
        })
        .collect::<RedisResult<Vec<_>>>()?;
    let seed = seeds[0].clone();
    let client = ClusterClient::builder(seeds)
        // Network errors may arrive after a write committed. Only follow explicit redirects.
        .retries(0)
        .retry_wait_formula(EXPONENT_FACTOR, EXPONENT_BASE)
        .min_retry_wait(min_wait)
        .max_retry_wait(max_wait)
        .connection_timeout(Duration::from_secs(5))
        .response_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| RedisError::InvalidConfiguration)?;
    Ok(Connection::cluster(client, seed, retries, prefix))
}
