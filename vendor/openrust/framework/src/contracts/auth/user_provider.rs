#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use crate::auth::{AuthFuture, AuthIdentifier, AuthResult, Credentials};

use super::Authenticatable;

/// Loads users, verifies credentials, and persists credential changes.
pub trait UserProvider: Send + Sync {
    /// Compiled model identity corresponding to an Eloquent provider's getModel.
    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent provider API")]
    fn getModel(&self) -> Option<std::any::TypeId> {
        None
    }

    fn retrieveById<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>;

    fn retrieveByToken<'a>(
        &'a self,
        id: &'a AuthIdentifier,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>;

    fn retrieve_by_api_token<'a>(
        &'a self,
        _column: &'a str,
        _token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok(None) })
    }

    fn updateRememberToken<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>>;

    fn retrieveByCredentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>;

    fn validateCredentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>>;

    fn rehashPasswordIfRequired<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> AuthFuture<'a, AuthResult<()>>;
}
