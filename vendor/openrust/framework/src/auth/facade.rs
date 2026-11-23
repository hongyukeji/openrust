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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Auth {
    pub fn manager() -> AuthResult<Arc<AuthManager>> {
        Self::get_facade_root().map_err(AuthError::backend)
    }

    /// Resolves a request's selected guard name, including shouldUse overrides.
    pub fn guardName(request: &Request, name: Option<&str>) -> AuthResult<String> {
        Self::manager()?.guard_name(request, name)
    }

    pub fn guard<'a>(
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<Box<dyn Guard + 'a>> {
        Self::manager()?.guard(request, name)
    }

    pub fn sessionGuard<'a>(
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<SessionGuard<'a>> {
        Self::manager()?.sessionGuard(request, name)
    }

    pub async fn user(request: &mut Request) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        Self::manager()?.user(request).await
    }

    /// Reads a cached request identity without invoking asynchronous authentication.
    pub fn resolvedUser(name: Option<&str>) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        Ok(Self::manager()?.resolvedUser(name))
    }

    pub async fn onceBasic(
        request: &mut Request,
        field: &str,
        conditions: &[(String, crate::database::Value)],
    ) -> AuthResult<()> {
        Self::sessionGuard(request, None)?
            .onceBasic(field, conditions)
            .await
    }

    pub async fn logoutOtherDevices(request: &mut Request, password: &str) -> AuthResult<()> {
        Self::sessionGuard(request, None)?
            .logoutOtherDevices(password)
            .await
    }

    pub async fn confirmPassword(request: &mut Request, password: &str) -> AuthResult<bool> {
        Self::sessionGuard(request, None)?
            .confirmPassword(password)
            .await
    }

    pub fn shouldUse(request: &mut Request, name: &str) -> AuthResult<()> {
        Self::manager()?.shouldUse(request, name)
    }
}
