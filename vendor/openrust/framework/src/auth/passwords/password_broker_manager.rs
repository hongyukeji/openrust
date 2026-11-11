use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use serde_json::Value;

use crate::{
    auth::{AuthError, AuthManager, AuthResult},
    cache::CacheManager,
    container::Container,
    contracts::{config::Repository, container::Container as _, hashing::Hasher},
    database::DatabaseManager,
    events::Dispatcher,
};

use super::{CacheTokenRepository, DatabaseTokenRepository, PasswordBroker, TokenRepository};

/// Resolves named password brokers from the application configuration.
pub struct PasswordBrokerManager {
    config: Arc<dyn Repository>,
    container: Weak<Container>,
    brokers: Mutex<HashMap<String, Arc<PasswordBroker>>>,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's password broker manager API"
)]
impl PasswordBrokerManager {
    pub(crate) fn new(config: Arc<dyn Repository>, container: Weak<Container>) -> Self {
        Self {
            config,
            container,
            brokers: Mutex::default(),
        }
    }

    pub fn getDefaultDriver(&self) -> String {
        self.config
            .get("auth.defaults.passwords")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "users".into())
    }

    pub fn broker(&self, name: Option<&str>) -> AuthResult<Arc<PasswordBroker>> {
        let default = self.getDefaultDriver();
        let name = name.filter(|name| !name.is_empty()).unwrap_or(&default);
        let mut brokers = self
            .brokers
            .lock()
            .expect("password broker manager lock poisoned");
        if let Some(broker) = brokers.get(name) {
            return Ok(broker.clone());
        }
        let broker = Arc::new(self.resolve(name)?);
        brokers.insert(name.to_owned(), broker.clone());
        Ok(broker)
    }

    fn resolve(&self, name: &str) -> AuthResult<PasswordBroker> {
        let config = self
            .config
            .get(&format!("auth.passwords.{name}"))
            .ok_or(AuthError::Configuration("password broker"))?;
        let app = self
            .container
            .upgrade()
            .ok_or(AuthError::Configuration("application scope"))?;
        let app_key = self
            .config
            .get("app.key")
            .ok_or(AuthError::Configuration("app.key"))?;
        let app_key = app_key
            .as_str()
            .ok_or(AuthError::Configuration("app.key"))?;
        let users = app
            .make::<AuthManager>()
            .map_err(AuthError::backend)?
            .createUserProvider(config["provider"].as_str().unwrap_or("users"))?;
        let hasher = app.make::<dyn Hasher>().map_err(AuthError::backend)?;
        let duration = |key, default| -> AuthResult<u64> {
            match config.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(value) => value
                    .as_u64()
                    .ok_or(AuthError::Configuration("password broker duration")),
            }
        };
        let expires = duration("expire", 60)?
            .checked_mul(60)
            .ok_or(AuthError::Configuration("password broker duration"))?;
        let expires = Duration::from_secs(expires);
        let throttle = Duration::from_secs(duration("throttle", 0)?);
        let tokens: Arc<dyn TokenRepository> = match config["driver"].as_str().unwrap_or("database")
        {
            "cache" => {
                let cache = app
                    .make::<CacheManager>()
                    .map_err(AuthError::backend)?
                    .store(config["store"].as_str())
                    .map_err(AuthError::backend)?;
                Arc::new(CacheTokenRepository::new(
                    cache, app_key, hasher, expires, throttle,
                )?)
            }
            "database" => {
                let connection = app
                    .make::<DatabaseManager>()
                    .map_err(AuthError::backend)?
                    .connection(config["connection"].as_str())
                    .map_err(AuthError::backend)?;
                let table = config["table"]
                    .as_str()
                    .ok_or(AuthError::Configuration("password token table"))?;
                Arc::new(DatabaseTokenRepository::new(
                    connection, table, app_key, hasher, expires, throttle,
                )?)
            }
            _ => return Err(AuthError::Configuration("password broker driver")),
        };
        let events = app.make::<Dispatcher>().map_err(AuthError::backend)?;
        let microseconds = self
            .config
            .get("auth.timebox_duration")
            .and_then(|value| value.as_u64())
            .unwrap_or(200_000);
        Ok(PasswordBroker::new(
            tokens,
            users,
            events,
            Duration::from_micros(microseconds),
        ))
    }
}
