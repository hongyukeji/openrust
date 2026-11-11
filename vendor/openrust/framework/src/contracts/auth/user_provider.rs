use crate::auth::{AuthFuture, AuthIdentifier, AuthResult, Credentials};

use super::Authenticatable;

/// Loads users, verifies credentials, and persists credential changes.
pub trait UserProvider: Send + Sync {
    fn retrieve_by_id<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>;

    fn retrieve_by_token<'a>(
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

    fn update_remember_token<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>>;

    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>;

    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>>;

    fn rehash_password_if_required<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> AuthFuture<'a, AuthResult<()>>;
}
