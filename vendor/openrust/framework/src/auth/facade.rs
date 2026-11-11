use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::auth::{Factory, Guard},
    http::Request,
    support::facades::Facade,
};

use super::{AuthError, AuthManager, AuthResult, SessionGuard};

/// Resolves the application's authentication factory.
pub struct Auth;

impl Facade for Auth {
    type Service = AuthManager;
    fn getFacadeAccessor() -> ServiceKey {
        "auth".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Auth API")]
impl Auth {
    pub fn manager() -> AuthResult<Arc<AuthManager>> {
        Self::getFacadeRoot().map_err(AuthError::backend)
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

    pub fn shouldUse(request: &mut Request, name: &str) -> AuthResult<()> {
        Self::manager()?.shouldUse(request, name)
    }
}
