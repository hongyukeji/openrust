use std::sync::Arc;

use crate::{
    auth::{AuthResult, Credentials},
    container::ServiceKey,
    contracts::auth::{ResetCallback, ResetLinkCallback},
    support::facades::Facade,
};

use super::{PasswordBroker, PasswordBrokerManager, PasswordResetStatus};

/// Accesses the application's password broker manager.
pub struct Password;

impl Facade for Password {
    type Service = PasswordBrokerManager;

    fn get_facade_accessor() -> ServiceKey {
        "auth.password".into()
    }
}

impl Password {
    pub fn broker(name: Option<&str>) -> AuthResult<Arc<PasswordBroker>> {
        Self::get_facade_root()?.broker(name)
    }

    pub async fn send_reset_link(
        credentials: &Credentials,
        callback: Option<&ResetLinkCallback<'_>>,
    ) -> AuthResult<PasswordResetStatus> {
        Self::broker(None)?
            .send_reset_link(credentials, callback)
            .await
    }

    pub async fn reset(
        credentials: &Credentials,
        token: &str,
        password: &[u8],
        callback: &ResetCallback<'_>,
    ) -> AuthResult<PasswordResetStatus> {
        Self::broker(None)?
            .reset(credentials, token, password, callback)
            .await
    }
}
