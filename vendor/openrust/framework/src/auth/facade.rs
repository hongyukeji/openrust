use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::auth::{Authenticatable, Factory, Guard},
    http::Request,
    support::facades::Facade,
};

use super::{AuthError, AuthManager, AuthResult, SessionGuard};

/// Resolves the application's authentication factory.
pub struct Auth;

impl Facade for Auth {
    type Service = AuthManager;
    fn get_facade_accessor() -> ServiceKey {
        "auth".into()
    }
}

impl Auth {
    pub fn manager() -> AuthResult<Arc<AuthManager>> {
        Self::get_facade_root().map_err(AuthError::backend)
    }

    /// 注册应用级异步会话身份策略；保留原生Guard、密码指纹和remember校验。
    pub fn add_session_validator(
        name: impl Into<String>,
        validate: impl for<'a> Fn(
            &'a str,
            &'a dyn Authenticatable,
            &'a crate::session::Store,
        ) -> super::AuthFuture<'a, AuthResult<bool>>
        + Send
        + Sync
        + 'static,
    ) -> AuthResult<()> {
        Self::manager()?.add_session_validator(name, validate)
    }

    pub fn remove_session_validator(name: &str) -> AuthResult<bool> {
        Ok(Self::manager()?.remove_session_validator(name))
    }

    /// Resolves a request's selected guard name, including shouldUse overrides.
    pub fn guard_name(request: &Request, name: Option<&str>) -> AuthResult<String> {
        Self::manager()?.guard_name(request, name)
    }

    pub fn guard<'a>(
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<Box<dyn Guard + 'a>> {
        Self::manager()?.guard(request, name)
    }

    pub fn session_guard<'a>(
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<SessionGuard<'a>> {
        Self::manager()?.session_guard(request, name)
    }

    pub async fn user(request: &mut Request) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        Self::manager()?.user(request).await
    }

    /// Reads a cached request identity without invoking asynchronous authentication.
    pub fn resolved_user(name: Option<&str>) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        Ok(Self::manager()?.resolved_user(name))
    }

    pub async fn once_basic(
        request: &mut Request,
        field: &str,
        conditions: &[(String, crate::database::Value)],
    ) -> AuthResult<()> {
        Self::session_guard(request, None)?
            .once_basic(field, conditions)
            .await
    }

    pub async fn logout_other_devices(request: &mut Request, password: &str) -> AuthResult<()> {
        Self::session_guard(request, None)?
            .logout_other_devices(password)
            .await
    }

    pub async fn confirm_password(request: &mut Request, password: &str) -> AuthResult<bool> {
        Self::session_guard(request, None)?
            .confirm_password(password)
            .await
    }

    pub fn should_use(request: &mut Request, name: &str) -> AuthResult<()> {
        Self::manager()?.should_use(request, name)
    }
}
