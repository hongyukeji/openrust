use std::path::Path;

use super::{config::Repository, container::Container};

/// Access to the application's root, container, and bootstrap state.
pub trait Application: Send + Sync {
    type Container: Container;

    fn base_path(&self) -> &Path;
    fn config(&self) -> &dyn Repository;
    fn container(&self) -> &Self::Container;
    fn is_booted(&self) -> bool;
}
