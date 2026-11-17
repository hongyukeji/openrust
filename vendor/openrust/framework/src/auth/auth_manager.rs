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
type ModelSerializer = fn(&dyn Authenticatable) -> AuthResult<serde_json::Map<String, Value>>;

#[derive(Default)]
struct Extensions {
    providers: HashMap<String, Arc<ProviderFactory>>,
    models: HashMap<String, (&'static str, Arc<ProviderFactory>)>,
    model_serializers: HashMap<std::any::TypeId, ModelSerializer>,
    requests: HashMap<String, Arc<Resolver>>,
    #[cfg(feature = "sanctum")]
    sanctum_models: std::collections::HashSet<std::any::TypeId>,
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
        let mut extensions = self.extensions.write().expect("auth extensions lock");
        extensions
            .model_serializers
            .insert(std::any::TypeId::of::<M>(), |user| {
                (user as &dyn std::any::Any)
                    .downcast_ref::<M>()
                    .expect("serializer registered for this model")
                    .toArray()
                    .map_err(AuthError::backend)
            });
        extensions.models.insert(
            name.into(),
            (
                std::any::type_name::<M>(),
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
            ),
        );
    }

    /// Serializes the authenticated identity through its registered model.
    pub fn userAttributes(
        &self,
        user: &dyn Authenticatable,
    ) -> AuthResult<serde_json::Map<String, Value>> {
        let serializer = self
            .extensions
            .read()
            .expect("auth extensions lock")
            .model_serializers
            .get(&user.type_id())
            .copied();
        let mut attributes = match serializer {
            Some(serialize) => serialize(user)?,
            None => [(
                user.get_auth_identifier_name().to_owned(),
                serde_json::to_value(user.get_auth_identifier()).map_err(AuthError::backend)?,
            )]
            .into_iter()
            .collect(),
        };
        attributes.remove(user.get_auth_password_name());
        attributes.remove(user.get_remember_token_name());
        Ok(attributes)
    }

    #[cfg(feature = "sanctum")]
    pub fn registerSanctumModel<M: crate::sanctum::HasApiTokens>(&self, name: impl Into<String>) {
        self.registerModel::<M>(name);
        self.extensions
            .write()
            .expect("auth extensions lock")
            .sanctum_models
            .insert(std::any::TypeId::of::<M>());
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn supports_sanctum_tokens(&self, user: &dyn Authenticatable) -> bool {
        self.extensions
            .read()
            .expect("auth extensions lock")
            .sanctum_models
            .contains(&user.type_id())
    }

    pub(crate) fn guard_uses_driver(
        &self,
        request: &Request,
        name: &str,
        driver: &str,
    ) -> AuthResult<bool> {
        let (_, config) = self.guard_config(request, Some(name))?;
        Ok(text(&config, "driver")? == driver)
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn same_registered_model(&self, configured: &str, actual: &str) -> bool {
        let extensions = self.extensions.read().expect("auth extensions lock");
        let models = &extensions.models;
        let configured_type = models.get(configured).map_or(configured, |(name, _)| *name);
        let actual_type = models.get(actual).map_or(actual, |(name, _)| *name);
        configured_type == actual_type
    }

    #[cfg(feature = "sanctum")]
    pub(crate) async fn retrieve_sanctum_owner(
        &self,
        model: &str,
        id: &super::AuthIdentifier,
    ) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        let factory = {
            let extensions = self.extensions.read().expect("auth extensions lock");
            extensions
                .models
                .get(model)
                .or_else(|| extensions.models.values().find(|(name, _)| *name == model))
                .map(|(_, factory)| factory.clone())
        };
        let Some(factory) = factory else {
            return Ok(None);
        };
        let container = self
            .container
            .upgrade()
            .ok_or(AuthError::Configuration("application scope"))?;
        factory(&container, &Value::Null)?.retrieve_by_id(id).await
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
            .insert(
                driver.into(),
                Arc::new(move |request| {
                    let result = resolve(request);
                    Box::pin(async move { result.await.map(|user| user.map(Arc::from)) })
                }),
            );
    }

    pub fn getDefaultDriver(&self) -> String {
        self.config
            .get("auth.defaults.guard")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "web".into())
    }

    pub(super) fn config(&self) -> &dyn Repository {
        self.config.as_ref()
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
        self.userForGuard(request, None).await
    }

    pub(crate) async fn userForGuard(
        &self,
        request: &mut Request,
        name: Option<&str>,
    ) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        let (name, _) = self.guard_config(request, name)?;
        self.guard(request, Some(&name))?.user().await?;
        Ok(AuthContext::state(request, &name).and_then(|state| state.user.clone()))
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn apply_test_identity(request: &mut Request) {
        use crate::sanctum::{AccessToken, RequestTokens, TestIdentity};
        let Some(identity) = request.parts_mut().extensions.remove::<TestIdentity>() else {
            return;
        };
        AuthContext::set_shared(request, &identity.guard, Some(identity.user), false);
        AuthContext::mutable(request).selected = Some(identity.guard.clone());
        RequestTokens::insert(
            request,
            &identity.guard,
            AccessToken::Test(identity.abilities),
        );
    }

    pub(crate) fn resolved_user(request: &Request, name: &str) -> Option<Arc<dyn Authenticatable>> {
        AuthContext::state(request, name)?.user.clone()
    }

    pub(crate) fn selected_guard(request: &Request) -> Option<&str> {
        AuthContext::get(request)?.selected.as_deref()
    }

    pub(crate) async fn database_session_user(
        &self,
        request: &mut Request,
    ) -> AuthResult<Option<crate::database::Value>> {
        if Self::selected_guard(request).is_none()
            && self
                .config
                .get(&format!("auth.guards.{}", self.getDefaultDriver()))
                .is_none_or(|config| config.is_null())
        {
            return Ok(None);
        }
        let id = self.guard(request, None)?.id().await?;
        Ok(Some(id.map_or(crate::database::Value::Null, Into::into)))
    }

    pub(crate) fn guard_name(&self, request: &Request, name: Option<&str>) -> AuthResult<String> {
        self.guard_config(request, name).map(|(name, _)| name)
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
        let config = self.config.get(&format!("auth.guards.{name}"));
        #[cfg(feature = "sanctum")]
        let config = if name == "sanctum" {
            let mut config = config.unwrap_or_else(|| serde_json::json!({}));
            if let Some(overrides) = config.as_object_mut() {
                overrides
                    .entry("driver")
                    .or_insert_with(|| Value::from("sanctum"));
                overrides.entry("provider").or_insert(Value::Null);
            }
            Some(config)
        } else {
            config
        };
        let config = config.ok_or(AuthError::Configuration("guard"))?;
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
        Ok(SessionGuard::new(name, provider, request).configure(
            minutes,
            rehash,
            self.container.clone(),
        ))
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
                    .map(|(_, factory)| factory.clone())
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
            #[cfg(feature = "sanctum")]
            "sanctum" => {
                let container = self
                    .container
                    .upgrade()
                    .ok_or(AuthError::Configuration("application scope"))?;
                let settings =
                    crate::sanctum::Guard::new(name.clone(), self.config.as_ref(), &config)?;
                let resolve: Arc<Resolver> = Arc::new(move |request| {
                    let settings = settings.clone();
                    let container = container.clone();
                    Box::pin(async move { settings.resolve(&container, request).await })
                });
                Ok(Box::new(RequestGuard::new(name, request, resolve)))
            }
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
