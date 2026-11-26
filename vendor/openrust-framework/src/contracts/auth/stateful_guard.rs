use crate::auth::{AuthFuture, AuthIdentifier, AuthResult, Credentials};

use super::{Authenticatable, Guard};

/// A guard capable of changing the authenticated session.
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
}
