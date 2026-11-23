use std::{
    collections::HashMap,
    sync::{Arc, RwLock, Weak},
};

use super::{Backend, MaintenanceError};
use crate::{
    container::Container,
    contracts::{
        config::Repository as Config,
        container::{BindingResolutionError, Container as _},
        foundation::MaintenanceMode,
    },
    support::driver_registry::DriverRegistry,
};

type Creator =
    dyn Fn(&Container) -> Result<Arc<dyn MaintenanceMode>, MaintenanceError> + Send + Sync;

/// 应用级维护驱动管理器，与 HTTP、命令和调度器共用驱动实例。
pub struct MaintenanceModeManager {
    container: RwLock<Weak<Container>>,
    config: Arc<dyn Config>,
    drivers: DriverRegistry<dyn MaintenanceMode>,
    creators: RwLock<HashMap<String, Arc<Creator>>>,
}

impl MaintenanceModeManager {
    pub fn new(container: &Arc<Container>) -> Result<Self, MaintenanceError> {
        Ok(Self::for_container(
            Arc::downgrade(container),
            container.get::<dyn Config>("config")?,
        ))
    }

    pub(crate) fn for_container(container: Weak<Container>, config: Arc<dyn Config>) -> Self {
        Self {
            container: RwLock::new(container),
            config,
            drivers: DriverRegistry::default(),
            creators: RwLock::default(),
        }
    }

    pub fn get_default_driver(&self) -> Result<String, MaintenanceError> {
        match self.config.get("app.maintenance.driver") {
            None => Ok("file".into()),
            Some(serde_json::Value::String(name)) => Ok(name),
            _ => Err(MaintenanceError::InvalidDriver),
        }
    }

    pub fn driver(&self, name: Option<&str>) -> Result<Arc<dyn MaintenanceMode>, MaintenanceError> {
        let name = match name.filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => self.get_default_driver()?,
        };
        self.drivers.get_or_resolve(
            &name,
            || {
                let container = self.get_container()?;
                let creator = self
                    .creators
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&name)
                    .cloned();
                if let Some(creator) = creator {
                    return creator(&container);
                }
                let backend = match name.as_str() {
                    "file" => Backend::File(container.make()?),
                    "array" => Backend::Array(RwLock::default()),
                    "cache" => {
                        let manager = container.make::<crate::cache::CacheManager>()?;
                        let configured = self.config.get("app.maintenance.store");
                        let selected = configured
                            .filter(|value| match value {
                                serde_json::Value::Null | serde_json::Value::Bool(false) => false,
                                serde_json::Value::String(name) => !name.is_empty() && name != "0",
                                serde_json::Value::Number(number) => number.as_f64() != Some(0.0),
                                serde_json::Value::Array(values) => !values.is_empty(),
                                _ => true,
                            })
                            .or_else(|| self.config.get("cache.default"));
                        let store = selected
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .ok_or(MaintenanceError::InvalidDriver)?;
                        Backend::Cache(manager, store)
                    }
                    _ => return Err(MaintenanceError::InvalidDriver),
                };
                Ok(Arc::new(backend) as Arc<dyn MaintenanceMode>)
            },
            MaintenanceError::DriverResolutionCycle,
        )
    }

    pub fn extend(
        &self,
        name: impl Into<String>,
        creator: impl Fn(&Container) -> Result<Arc<dyn MaintenanceMode>, MaintenanceError>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        let retired = self
            .creators
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.into(), Arc::new(creator));
        drop(retired);
        self
    }

    pub fn get_drivers(&self) -> HashMap<String, Arc<dyn MaintenanceMode>> {
        self.drivers.entries()
    }

    pub fn get_container(&self) -> Result<Arc<Container>, MaintenanceError> {
        self.container
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
            .ok_or(MaintenanceError::Unavailable(
                BindingResolutionError::FacadeNotSet,
            ))
    }

    /// 仅更换容器；已捕获配置、驱动与创建器保持不变。
    pub fn set_container(&self, container: &Arc<Container>) -> &Self {
        *self
            .container
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(container);
        self
    }

    pub fn forget_drivers(&self) -> &Self {
        self.drivers.forget_all();
        self
    }
}
