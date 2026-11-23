#![allow(non_snake_case)]

use crate::auth::{AuthFuture, AuthResult};

use super::Authenticatable;

/// User capability for applications that require email verification.
pub trait MustVerifyEmail: Authenticatable {
    fn has_verified_email(&self) -> bool;
    /// Returns true only when this exact email changes from unverified to verified.
    fn mark_email_as_verified(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn mark_email_as_unverified(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn send_email_verification_notification(&self) -> AuthFuture<'_, AuthResult<()>>;
    fn get_email_for_verification(&self) -> &str;
}
