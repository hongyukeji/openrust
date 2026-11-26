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
    bindings: Mutex<HashMap<String, Arc<super::user_bindings::UserBindings>>>,
}

impl PasswordBrokerManager {
    pub(crate) fn new(config: Arc<dyn Repository>, container: Weak<Container>) -> Self {
        Self {
            config,
            container,
            brokers: Mutex::default(),
            bindings: Mutex::default(),
        }
    }

    pub fn get_default_driver(&self) -> String {
        self.config
            .get("auth.defaults.passwords")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "users".into())
    }

    /// 为某类用户追加具名快照规则；同名规则替换，其他规则与已缓存Broker均保留。
    pub fn bind_user_tokens_using<M: crate::contracts::auth::Authenticatable>(
        &self,
        name: Option<&str>,
        key: &str,
        callback: impl Fn(&M) -> AuthResult<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        let default = self.get_default_driver();
        let name = name.filter(|value| !value.is_empty()).unwrap_or(&default);
        self.user_bindings(name).register::<M>(key, callback)
    }
    fn user_bindings(&self, name: &str) -> Arc<super::user_bindings::UserBindings> {
        self.bindings
            .lock()
            .expect("password user binding registry lock")
            .entry(name.into())
            .or_default()
            .clone()
    }

    pub fn broker(&self, name: Option<&str>) -> AuthResult<Arc<PasswordBroker>> {
        let default = self.get_default_driver();
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
            .make::<AuthManager>()?
            .create_user_provider(config["provider"].as_str().unwrap_or("users"))?;
        let hasher = app.make::<dyn Hasher>()?;
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
                    .make::<CacheManager>()?
                    .store(config["store"].as_str())?;
                Arc::new(
                    CacheTokenRepository::new(cache, app_key, hasher, expires, throttle)?
                        .with_user_bindings(self.user_bindings(name)),
                )
            }
            "database" => {
                let connection = app
                    .make::<DatabaseManager>()?
                    .connection(config["connection"].as_str())?;
                let table = config["table"]
                    .as_str()
                    .ok_or(AuthError::Configuration("password token table"))?;
                Arc::new(
                    DatabaseTokenRepository::new(
                        connection, table, app_key, hasher, expires, throttle,
                    )?
                    .with_user_bindings(self.user_bindings(name)),
                )
            }
            _ => return Err(AuthError::Configuration("password broker driver")),
        };
        let events = app.make::<Dispatcher>()?;
        let microseconds = self
            .config
            .get("auth.timebox_duration")
            .and_then(|value| value.as_u64())
            .unwrap_or(200_000);
        Ok(
            PasswordBroker::new(tokens, users, events, Duration::from_micros(microseconds))
                .with_user_bindings(self.user_bindings(name)),
        )
    }
}
