use crate::{auth::AuthIdentifier, contracts::events::Event};

/// The default reset notification was sent for this user.
pub struct PasswordResetLinkSent {
    pub user: AuthIdentifier,
}

impl Event for PasswordResetLinkSent {
    fn name(&self) -> &str {
        "PasswordResetLinkSent"
    }
}
