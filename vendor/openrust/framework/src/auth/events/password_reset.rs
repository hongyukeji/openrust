use crate::{auth::AuthIdentifier, contracts::events::Event};

/// Emitted by the application after persisting a new password.
pub struct PasswordReset {
    pub user: AuthIdentifier,
}

impl Event for PasswordReset {
    fn name(&self) -> &str {
        "PasswordReset"
    }
}
