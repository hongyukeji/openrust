use std::sync::Arc;

use crate::contracts::{auth::Authenticatable, events::Event};

/// A new account was registered.
pub struct Registered {
    pub user: Arc<dyn Authenticatable>,
}

impl Event for Registered {
    fn name(&self) -> &str {
        "Registered"
    }
}
