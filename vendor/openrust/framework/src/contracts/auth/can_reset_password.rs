#![allow(non_snake_case)]

use crate::auth::{AuthFuture, AuthResult};

use super::Authenticatable;

/// User capability required by the password broker.
pub trait CanResetPassword: Authenticatable {
    fn getEmailForPasswordReset(&self) -> &str;
    fn sendPasswordResetNotification<'a>(
        &'a self,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>>;
}
