use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use ::redis::{Client, ConnectionAddr, IntoConnectionInfo, aio::ConnectionManagerConfig};
use serde::Deserialize;

use crate::contracts::{config::Repository, redis::Factory};

use super::{RedisError, RedisResult, connections::Connection};

/// Resolves named Redis connections and manages their lifecycle.
pub struct RedisManager {
    config: Arc<dyn Repository>,
    connections: Mutex<HashMap<String, Arc<Connection>>>,
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
        }
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
        let value = config
            .get(name)
            .cloned()
            .filter(|_| !matches!(name, "options" | "clusters" | "client"))
            .ok_or_else(|| RedisError::UnknownConnection { name: name.into() })?;
        let settings: Settings =
            serde_json::from_value(value).map_err(|_| RedisError::InvalidConfiguration)?;
        let prefix = match config.pointer("/options/prefix") {
            None | Some(serde_json::Value::Null) => String::new(),
            Some(serde_json::Value::String(prefix)) => prefix.clone(),
            _ => return Err(RedisError::InvalidConfiguration),
        };
        let connection = Arc::new(settings.connect(prefix)?);
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
        let client = Client::open(info).map_err(|_| RedisError::InvalidConfiguration)?;
        let config = ConnectionManagerConfig::new()
            .set_number_of_retries(self.max_retries)
            .set_min_delay(Duration::from_millis(self.backoff_base))
            .set_max_delay(Duration::from_millis(self.backoff_cap))
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(Some(Duration::from_secs(5)));
        Ok(Connection::new(client, config, prefix))
    }
}
