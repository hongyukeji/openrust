#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::{
    Error, PendingScopedFeatureInteraction, Result, Scope,
    contracts::Definitions,
    drivers::{ArrayDriver, DatabaseDriver, Decorator},
};
use openrust::database::DatabaseManager;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(crate) struct ScopedStores(pub RwLock<BTreeMap<String, Arc<Decorator>>>);
type DriverCreator = Arc<
    dyn Fn(&FeatureManager, &serde_json::Value) -> Result<Arc<dyn crate::Driver>> + Send + Sync,
>;
type ScopeResolver = Arc<dyn Fn(&str) -> Result<Scope> + Send + Sync>;
#[derive(Default)]
pub(crate) struct FeatureRegistry {
    definitions: RwLock<BTreeMap<String, Definitions>>,
    defaultDriver: RwLock<Option<String>>,
    customCreators: RwLock<BTreeMap<String, DriverCreator>>,
    classes: RwLock<BTreeMap<String, Arc<crate::contracts::FeatureClasses>>>,
    scopeResolver: RwLock<Option<ScopeResolver>>,
    useMorphMap: Arc<AtomicBool>,
}
pub struct FeatureManager {
    config: Arc<dyn openrust::contracts::config::Repository>,
    database: Arc<DatabaseManager>,
    auth: Arc<openrust::auth::AuthManager>,
    events: Arc<openrust::events::Dispatcher>,
    stores: Arc<ScopedStores>,
    registry: Arc<FeatureRegistry>,
}
impl FeatureManager {
    pub(crate) fn new(
        config: Arc<dyn openrust::contracts::config::Repository>,
        database: Arc<DatabaseManager>,
        auth: Arc<openrust::auth::AuthManager>,
        events: Arc<openrust::events::Dispatcher>,
        registry: Arc<FeatureRegistry>,
    ) -> Self {
        Self {
            config,
            database,
            auth,
            events,
            registry,
            stores: Arc::new(ScopedStores::default()),
        }
    }
    pub fn get_default_driver(&self) -> Result<String> {
        if let Some(name) = self
            .registry
            .defaultDriver
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .as_ref()
        {
            return Ok(name.clone());
        }
        self.config
            .get("pennant.default")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(Error::InvalidConfiguration)
    }
    pub fn setDefaultDriver(&self, name: &str) -> Result<()> {
        *self
            .registry
            .defaultDriver
            .write()
            .map_err(|_| Error::StateUnavailable)? = Some(name.into());
        Ok(())
    }
    pub fn store(&self, name: Option<&str>) -> Result<Arc<Decorator>> {
        self.driver(name)
    }
    pub fn driver(&self, name: Option<&str>) -> Result<Arc<Decorator>> {
        let name = name
            .map(str::to_owned)
            .map(Ok)
            .unwrap_or_else(|| self.get_default_driver())?;
        if let Some(store) = self
            .stores
            .0
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(&name)
        {
            return Ok(store.clone());
        }
        let config = self
            .config
            .get(&format!("pennant.stores.{name}"))
            .ok_or(Error::InvalidConfiguration)?;
        let definitions = self
            .registry
            .definitions
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .entry(name.clone())
            .or_default()
            .clone();
        let driverName = config
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::InvalidConfiguration)?;
        let creator = self
            .registry
            .customCreators
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .get(driverName)
            .cloned();
        let driver: Arc<dyn crate::Driver> = if let Some(creator) = creator {
            creator(self, &config)?
        } else {
            match Some(driverName) {
                Some("array") => {
                    let mut driver = ArrayDriver::new();
                    driver.resolvers = definitions;
                    driver.events = Some(self.events.clone());
                    driver.useMorphMap = self.registry.useMorphMap.clone();
                    Arc::new(driver)
                }
                Some("database") => {
                    let connection = match config.get("connection") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(name)) => Some(name.as_str()),
                        _ => return Err(Error::InvalidConfiguration),
                    };
                    let connection = self.database.connection(connection)?;
                    let table = match config.get("table") {
                        None | Some(serde_json::Value::Null) => "features",
                        Some(serde_json::Value::String(table)) => table.as_str(),
                        _ => return Err(Error::InvalidConfiguration),
                    };
                    let mut driver = DatabaseDriver::new(connection).withTable(table)?;
                    if let Some(timezone) = self.config.get("app.timezone") {
                        driver = driver
                            .withTimezone(timezone.as_str().ok_or(Error::InvalidConfiguration)?)?;
                    }
                    driver.resolvers = definitions;
                    driver.events = Some(self.events.clone());
                    driver.useMorphMap = self.registry.useMorphMap.clone();
                    Arc::new(driver)
                }
                _ => return Err(Error::InvalidConfiguration),
            }
        };
        let mut decorator = Decorator::new(&name, driver);
        decorator.classes = self
            .registry
            .classes
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .entry(name.clone())
            .or_default()
            .clone();
        decorator.events = Some(self.events.clone());
        decorator.useMorphMap = self.registry.useMorphMap.clone();
        let decorator = Arc::new(decorator);
        let mut stores = self.stores.0.write().map_err(|_| Error::StateUnavailable)?;
        Ok(stores.entry(name).or_insert(decorator).clone())
    }
    pub fn extend(
        &self,
        driver: &str,
        creator: impl Fn(&FeatureManager, &serde_json::Value) -> Result<Arc<dyn crate::Driver>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.registry
            .customCreators
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .insert(driver.into(), Arc::new(creator));
        Ok(())
    }
    pub fn forgetDriver(&self, name: Option<&str>) -> Result<()> {
        let name = name
            .map(str::to_owned)
            .map(Ok)
            .unwrap_or_else(|| self.get_default_driver())?;
        self.stores
            .0
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(&name);
        self.registry
            .definitions
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(&name);
        self.registry
            .classes
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .remove(&name);
        Ok(())
    }
    pub fn forgetDrivers(&self) -> Result<()> {
        self.stores
            .0
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .clear();
        self.registry
            .definitions
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .clear();
        self.registry
            .classes
            .write()
            .map_err(|_| Error::StateUnavailable)?
            .clear();
        Ok(())
    }
    pub fn flushCache(&self) -> Result<()> {
        for store in self
            .stores
            .0
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .values()
        {
            store.flushCache()?;
        }
        Ok(())
    }
    pub fn useMorphMap(&self, value: bool) {
        self.registry.useMorphMap.store(value, Ordering::Relaxed);
    }
    pub fn serializeScope(&self, scope: &Scope) -> String {
        scope.serialize(self.registry.useMorphMap.load(Ordering::Relaxed))
    }
    pub fn resolveScopeUsing(
        &self,
        resolver: impl Fn(&str) -> Result<Scope> + Send + Sync + 'static,
    ) -> Result<()> {
        *self
            .registry
            .scopeResolver
            .write()
            .map_err(|_| Error::StateUnavailable)? = Some(Arc::new(resolver));
        Ok(())
    }
    pub fn defaultScope(&self, store: &str) -> Result<Scope> {
        let resolver = self
            .registry
            .scopeResolver
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .clone();
        if let Some(resolver) = resolver {
            return resolver(store);
        }
        let Some(user) = self.auth.resolved_user(None) else {
            return Ok(Scope::Null);
        };
        let class = self
            .auth
            .user_class(user.as_ref())
            .ok_or(Error::InvalidScope)?;
        let morph = if self.registry.useMorphMap.load(Ordering::Relaxed) {
            self.auth
                .user_morph_class(user.as_ref())
                .map_err(|_| Error::InvalidScope)?
                .map(str::to_owned)
        } else {
            None
        };
        Ok(Scope::Model {
            class: class.into(),
            morph,
            key: user.get_auth_identifier().to_string(),
        })
    }
    /// Resolves a lazy guard when the caller has the native HTTP request.
    pub async fn for_request(
        &self,
        request: &mut openrust::http::Request,
    ) -> Result<PendingScopedFeatureInteraction> {
        let store = self.store(None)?;
        let custom = self
            .registry
            .scopeResolver
            .read()
            .map_err(|_| Error::StateUnavailable)?
            .is_some();
        if !custom {
            self.auth.user(request).await.map_err(Error::Auth)?;
        }
        Ok(store.r#for(self.defaultScope(store.storeName())?))
    }
    pub fn r#for(&self, scope: impl Into<Scope>) -> Result<PendingScopedFeatureInteraction> {
        Ok(self.store(None)?.r#for(scope))
    }
    pub fn globally(&self) -> Result<PendingScopedFeatureInteraction> {
        Ok(self.store(None)?.globally())
    }
    pub fn interaction(&self) -> Result<PendingScopedFeatureInteraction> {
        let store = self.store(None)?;
        let scope = self.defaultScope(store.storeName())?;
        Ok(store.r#for(scope))
    }
}
