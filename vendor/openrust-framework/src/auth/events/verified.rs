use crate::{auth::AuthIdentifier, contracts::events::Event};

/// The user's email address was verified.
pub struct Verified {
    pub user: AuthIdentifier,
}

impl Event for Verified {
    fn name(&self) -> &str {
        "Verified"
    }
}
