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

    fn getFacadeAccessor() -> ServiceKey {
        "auth.password".into()
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Password {
    pub fn broker(name: Option<&str>) -> AuthResult<Arc<PasswordBroker>> {
        Self::getFacadeRoot()?.broker(name)
    }

    pub async fn sendResetLink(
        credentials: &Credentials,
        callback: Option<&ResetLinkCallback<'_>>,
    ) -> AuthResult<PasswordResetStatus> {
        Self::broker(None)?
            .sendResetLink(credentials, callback)
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
