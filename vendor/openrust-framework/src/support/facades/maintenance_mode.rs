use super::Facade;
use crate::{
    container::{Container, ServiceKey},
    contracts::foundation::MaintenanceMode as Driver,
    foundation::{
        MaintenanceModeManager,
        maintenance::{MaintenanceError, MaintenancePayload},
    },
};
use std::{collections::HashMap, sync::Arc};

/// 维护模式门面，委托应用实际使用的维护驱动管理器。
pub struct MaintenanceMode;
impl Facade for MaintenanceMode {
    type Service = MaintenanceModeManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<MaintenanceModeManager>()
    }
}
impl MaintenanceMode {
    pub fn get_default_driver() -> Result<String, MaintenanceError> {
        Self::get_facade_root()?.get_default_driver()
    }
    pub fn driver(name: Option<&str>) -> Result<Arc<dyn Driver>, MaintenanceError> {
        Self::get_facade_root()?.driver(name)
    }
    pub fn extend(
        name: impl Into<String>,
        creator: impl Fn(&Container) -> Result<Arc<dyn Driver>, MaintenanceError>
        + Send
        + Sync
        + 'static,
    ) -> Result<Arc<MaintenanceModeManager>, MaintenanceError> {
        let manager = Self::get_facade_root()?;
        manager.extend(name, creator);
        Ok(manager)
    }
    pub fn get_drivers() -> Result<HashMap<String, Arc<dyn Driver>>, MaintenanceError> {
        Ok(Self::get_facade_root()?.get_drivers())
    }
    pub fn get_container() -> Result<Arc<Container>, MaintenanceError> {
        Self::get_facade_root()?.get_container()
    }
    pub fn set_container(
        container: &Arc<Container>,
    ) -> Result<Arc<MaintenanceModeManager>, MaintenanceError> {
        let manager = Self::get_facade_root()?;
        manager.set_container(container);
        Ok(manager)
    }
    pub fn forget_drivers() -> Result<Arc<MaintenanceModeManager>, MaintenanceError> {
        let manager = Self::get_facade_root()?;
        manager.forget_drivers();
        Ok(manager)
    }
    pub async fn activate(payload: &MaintenancePayload) -> Result<(), MaintenanceError> {
        Self::driver(None)?.activate(payload).await
    }
    pub async fn deactivate() -> Result<(), MaintenanceError> {
        Self::driver(None)?.deactivate().await
    }
    pub async fn active() -> Result<bool, MaintenanceError> {
        Self::driver(None)?.active().await
    }
    pub async fn data() -> Result<Option<MaintenancePayload>, MaintenanceError> {
        Self::driver(None)?.data().await
    }
}
