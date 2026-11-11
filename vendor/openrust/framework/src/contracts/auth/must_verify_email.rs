#![allow(non_snake_case)]

use crate::auth::{AuthFuture, AuthResult};

use super::Authenticatable;

/// User capability for applications that require email verification.
pub trait MustVerifyEmail: Authenticatable {
    fn hasVerifiedEmail(&self) -> bool;
    fn markEmailAsVerified(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn markEmailAsUnverified(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn sendEmailVerificationNotification(&self) -> AuthFuture<'_, AuthResult<()>>;
    fn getEmailForVerification(&self) -> &str;
}
