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
    /// 应用扩展的具名用户绑定；原生应用未注册时不改变Token格式或验证方式。
    pub fn bind_user_tokens_using<M: crate::contracts::auth::Authenticatable>(
        name: Option<&str>,
        key: &str,
        callback: impl Fn(&M) -> AuthResult<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        Self::get_facade_root()?.bind_user_tokens_using::<M>(name, key, callback)
    }

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
