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
