use crate::auth::{AuthFuture, AuthIdentifier, AuthResult, Credentials};

use super::Authenticatable;

/// Resolves the current user and checks credentials.
pub trait Guard: Send {
    fn user(&mut self) -> AuthFuture<'_, AuthResult<Option<&dyn Authenticatable>>>;
    fn id(&mut self) -> AuthFuture<'_, AuthResult<Option<AuthIdentifier>>>;
    fn check(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn guest(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn has_user(&self) -> bool;
    fn validate<'a>(&'a mut self, credentials: &'a Credentials)
    -> AuthFuture<'a, AuthResult<bool>>;
    fn set_user(&mut self, user: Box<dyn Authenticatable>);
}
