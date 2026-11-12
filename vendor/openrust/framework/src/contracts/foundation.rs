use std::{path::Path, sync::Arc};

use super::{
    config::Repository,
    container::{BindingResolutionError, Container},
};

/// Access to the application's root, container, and bootstrap state.
pub trait Application: Send + Sync {
    type Container: Container;

    fn base_path(&self) -> &Path;
    fn config(&self) -> Result<Arc<dyn Repository>, BindingResolutionError>;
    fn container(&self) -> &Self::Container;
    fn is_booted(&self) -> bool;
}

/// Stores an application's maintenance status and response data.
pub trait MaintenanceMode: Send + Sync {
    fn activate<'a>(
        &'a self,
        payload: &'a crate::foundation::maintenance::MaintenancePayload,
    ) -> crate::foundation::maintenance::MaintenanceFuture<'a, ()>;
    fn deactivate(&self) -> crate::foundation::maintenance::MaintenanceFuture<'_, ()>;
    fn active(&self) -> crate::foundation::maintenance::MaintenanceFuture<'_, bool>;
    fn data(
        &self,
    ) -> crate::foundation::maintenance::MaintenanceFuture<
        '_,
        Option<crate::foundation::maintenance::MaintenancePayload>,
    >;
}
