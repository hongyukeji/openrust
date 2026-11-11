use std::{
    collections::HashMap,
    sync::{Arc, RwLock, Weak},
};

use serde_json::Value;

use crate::{
    container::Container,
    contracts::{
        auth::{Authenticatable, Factory, Guard, UserProvider},
        config::Repository,
        container::Container as _,
        hashing::Hasher,
    },
    database::{DatabaseManager, eloquent::Model},
    http::Request,
};

use super::{
    AuthError, AuthFuture, AuthResult, DatabaseUserProvider, EloquentUserProvider, RequestGuard,
    SessionGuard, TokenGuard, request_guard::Resolver, session_guard::AuthContext,
};

type ProviderFactory =
    dyn Fn(&Container, &Value) -> AuthResult<Arc<dyn UserProvider>> + Send + Sync;

#[derive(Default)]
struct Extensions {
    providers: HashMap<String, Arc<ProviderFactory>>,
    models: HashMap<String, Arc<ProviderFactory>>,
    requests: HashMap<String, Arc<Resolver>>,
}

/// Application-level guard definitions. Guard state lives in the HTTP request.
pub struct AuthManager {
    container: Weak<Container>,
    config: Arc<dyn Repository>,
    extensions: RwLock<Extensions>,
}

#[allow(non_snake_case, reason = "Matches Laravel's AuthManager API")]
impl AuthManager {
    pub(crate) fn new(container: Weak<Container>, config: Arc<dyn Repository>) -> Self {
        Self {
            container,
            config,
            extensions: RwLock::default(),
        }
    }

    pub fn provider(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(&Container, &Value) -> AuthResult<Arc<dyn UserProvider>>
        + Send
        + Sync
        + 'static,
    ) {
        self.extensions
            .write()
            .expect("auth extensions lock")
            .providers
            .insert(driver.into(), Arc::new(factory));
    }

    pub fn registerModel<M: Model + Authenticatable>(&self, name: impl Into<String>) {
        self.extensions
            .write()
            .expect("auth extensions lock")
            .models
            .insert(
                name.into(),
                Arc::new(|container, config| {
                    let manager = container
                        .make::<DatabaseManager>()
                        .map_err(AuthError::backend)?;
                    let prototype = M::from_state(Default::default());
                    let name = config
                        .get("connection")
                        .and_then(Value::as_str)
                        .or_else(|| prototype.getConnectionName());
                    let connection = manager.connection(name).map_err(AuthError::backend)?;
                    let hasher = container.make::<dyn Hasher>().map_err(AuthError::backend)?;
                    Ok(Arc::new(EloquentUserProvider::<M>::new(connection, hasher)))
                }),
            );
    }

    pub fn viaRequest(
        &self,
        driver: impl Into<String>,
        resolve: impl for<'a> Fn(
            &'a mut Request,
        )
            -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>
        + Send
        + Sync
        + 'static,
    ) {
        self.extensions
            .write()
            .expect("auth extensions lock")
            .requests
            .insert(driver.into(), Arc::new(resolve));
    }

    pub fn getDefaultDriver(&self) -> String {
        self.config
            .get("auth.defaults.guard")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "web".into())
    }

    pub fn shouldUse(&self, request: &mut Request, name: &str) -> AuthResult<()> {
        self.should_use(request, name)
    }

    pub fn sessionGuard<'a>(
        &self,
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<SessionGuard<'a>> {
        let (name, config) = self.guard_config(request, name)?;
        if text(&config, "driver")? != "session" {
            return Err(AuthError::Configuration("stateful guard driver"));
        }
        self.session(request, name, &config)
    }

    /// Resolves the selected identity without borrowing the request after lookup.
    pub async fn user(
        &self,
        request: &mut Request,
    ) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        let (name, _) = self.guard_config(request, None)?;
        self.guard(request, Some(&name))?.user().await?;
        Ok(AuthContext::state(request, &name).and_then(|state| state.user.clone()))
    }

    fn guard_config(&self, request: &Request, name: Option<&str>) -> AuthResult<(String, Value)> {
        let name = name
            .map(str::to_owned)
            .or_else(|| AuthContext::get(request).and_then(|context| context.selected.clone()))
            .unwrap_or_else(|| self.getDefaultDriver());
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(AuthError::Configuration("guard name"));
        }
        let config = self
            .config
            .get(&format!("auth.guards.{name}"))
            .ok_or(AuthError::Configuration("guard"))?;
        Ok((name, config))
    }

    fn session<'a>(
        &self,
        request: &'a mut Request,
        name: String,
        config: &Value,
    ) -> AuthResult<SessionGuard<'a>> {
        let provider = self.createUserProvider(text(config, "provider")?)?;
        let minutes = config
            .get("remember_duration")
            .and_then(Value::as_i64)
            .unwrap_or(576_000);
        if minutes <= 0 {
            return Err(AuthError::Configuration("remember duration"));
        }
        let rehash = self
            .config
            .get("hashing.rehash_on_login")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        Ok(SessionGuard::new(name, provider, request).configure(minutes, rehash))
    }

    pub fn createUserProvider(&self, name: &str) -> AuthResult<Arc<dyn UserProvider>> {
        let config = self
            .config
            .get(&format!("auth.providers.{name}"))
            .ok_or(AuthError::Configuration("user provider"))?;
        let driver = text(&config, "driver")?;
        let container = self
            .container
            .upgrade()
            .ok_or(AuthError::Configuration("application scope"))?;
        let factory = self
            .extensions
            .read()
            .expect("auth extensions lock")
            .providers
            .get(driver)
            .cloned();
        if let Some(factory) = factory {
            return factory(&container, &config);
        }
        match driver {
            "eloquent" => {
                let model = text(&config, "model")?;
                let factory = self
                    .extensions
                    .read()
                    .expect("auth extensions lock")
                    .models
                    .get(model)
                    .cloned()
                    .ok_or(AuthError::Configuration("compiled Eloquent model"))?;
                factory(&container, &config)
            }
            "database" => {
                let manager = container
                    .make::<DatabaseManager>()
                    .map_err(AuthError::backend)?;
                let connection = manager
                    .connection(config.get("connection").and_then(Value::as_str))
                    .map_err(AuthError::backend)?;
                let hasher = container.make::<dyn Hasher>().map_err(AuthError::backend)?;
                let provider =
                    DatabaseUserProvider::new(connection, hasher, text(&config, "table")?).columns(
                        config
                            .get("identifier")
                            .and_then(Value::as_str)
                            .unwrap_or("id"),
                        config
                            .get("password")
                            .and_then(Value::as_str)
                            .unwrap_or("password"),
                        config
                            .get("remember_token")
                            .and_then(Value::as_str)
                            .unwrap_or("remember_token"),
                    );
                Ok(Arc::new(provider))
            }
            _ => Err(AuthError::Configuration("user provider driver")),
        }
    }
}

impl Factory for AuthManager {
    fn guard<'a>(
        &self,
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<Box<dyn Guard + 'a>> {
        let (name, config) = self.guard_config(request, name)?;
        let driver = text(&config, "driver")?;
        match driver {
            "session" => Ok(Box::new(self.session(request, name, &config)?)),
            "token" => {
                let provider = self.createUserProvider(text(&config, "provider")?)?;
                let column = config
                    .get("storage_key")
                    .and_then(Value::as_str)
                    .unwrap_or("api_token")
                    .to_owned();
                let hash = config.get("hash").and_then(Value::as_bool).unwrap_or(false);
                Ok(Box::new(TokenGuard::bind(
                    name, request, provider, column, hash,
                )))
            }
            driver => {
                let resolve = self
                    .extensions
                    .read()
                    .expect("auth extensions lock")
                    .requests
                    .get(driver)
                    .cloned()
                    .ok_or(AuthError::Configuration("guard driver"))?;
                Ok(Box::new(RequestGuard::new(name, request, resolve)))
            }
        }
    }

    fn should_use(&self, request: &mut Request, name: &str) -> AuthResult<()> {
        self.guard_config(request, Some(name))?;
        AuthContext::mutable(request).selected = Some(name.to_owned());
        Ok(())
    }
}

fn text<'a>(config: &'a Value, key: &'static str) -> AuthResult<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(AuthError::Configuration(key))
}
