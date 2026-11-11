use crate::auth::{AuthFuture, AuthIdentifier, AuthResult, Credentials};

use super::{Authenticatable, Guard};

/// A guard capable of changing the authenticated session.
#[allow(non_snake_case, reason = "Matches Laravel's StatefulGuard API")]
pub trait StatefulGuard: Guard {
    fn attempt<'a>(
        &'a mut self,
        credentials: &'a Credentials,
        remember: bool,
    ) -> AuthFuture<'a, AuthResult<bool>>;

    fn once<'a>(&'a mut self, credentials: &'a Credentials) -> AuthFuture<'a, AuthResult<bool>>;

    fn login(
        &mut self,
        user: Box<dyn Authenticatable>,
        remember: bool,
    ) -> AuthFuture<'_, AuthResult<()>>;

    fn login_using_id<'a>(
        &'a mut self,
        id: &'a AuthIdentifier,
        remember: bool,
    ) -> AuthFuture<'a, AuthResult<Option<&'a dyn Authenticatable>>>;

    fn once_using_id<'a>(&'a mut self, id: &'a AuthIdentifier) -> AuthFuture<'a, AuthResult<bool>>;
    fn via_remember(&self) -> bool;
    fn logout(&mut self) -> AuthFuture<'_, AuthResult<()>>;

    fn loginUsingId<'a>(
        &'a mut self,
        id: &'a AuthIdentifier,
        remember: bool,
    ) -> AuthFuture<'a, AuthResult<Option<&'a dyn Authenticatable>>> {
        self.login_using_id(id, remember)
    }
    fn onceUsingId<'a>(&'a mut self, id: &'a AuthIdentifier) -> AuthFuture<'a, AuthResult<bool>> {
        self.once_using_id(id)
    }
    fn viaRemember(&self) -> bool {
        self.via_remember()
    }
}
