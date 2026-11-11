#![allow(non_snake_case)]

use crate::auth::{AuthFuture, AuthResult, Credentials, passwords::PasswordResetStatus};

use super::CanResetPassword;

/// An override for the broker's default reset notification.
pub type ResetLinkCallback<'scope> = dyn for<'a> Fn(
        &'a dyn CanResetPassword,
        &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<PasswordResetStatus>>>
    + Send
    + Sync
    + 'scope;

/// Persists the new password for a user whose reset token has been validated.
pub type ResetCallback<'scope> = dyn for<'a> Fn(&'a mut dyn CanResetPassword, &'a [u8]) -> AuthFuture<'a, AuthResult<()>>
    + Send
    + Sync
    + 'scope;

/// Application-facing password reset operations.
pub trait PasswordBroker: Send + Sync {
    fn sendResetLink<'a>(
        &'a self,
        credentials: &'a Credentials,
        callback: Option<&'a ResetLinkCallback<'a>>,
    ) -> AuthFuture<'a, AuthResult<PasswordResetStatus>>;
    fn reset<'a>(
        &'a self,
        credentials: &'a Credentials,
        token: &'a str,
        password: &'a [u8],
        callback: &'a ResetCallback<'a>,
    ) -> AuthFuture<'a, AuthResult<PasswordResetStatus>>;
}
