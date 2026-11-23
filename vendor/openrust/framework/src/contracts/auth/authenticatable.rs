#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::any::Any;

use crate::{
    auth::{AuthIdentifier, AuthResult},
    database::Value,
};

/// Identity and credential fields exposed by an authenticatable user.
pub trait Authenticatable: Any + Send + Sync {
    fn getAuthIdentifierName(&self) -> &str;
    fn getAuthIdentifier(&self) -> AuthIdentifier;
    fn getAuthPasswordName(&self) -> &str;
    fn getAuthPassword(&self) -> Option<&str>;
    fn getRememberToken(&self) -> Option<&str>;
    fn setRememberToken(&mut self, token: Option<String>);
    fn getRememberTokenName(&self) -> &str;

    /// Reads a named attribute used by authentication-aware middleware.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn getAuthAttribute(&self, _name: &str) -> AuthResult<Option<Value>> {
        Ok(None)
    }

    fn as_password_reset(&self) -> Option<&dyn super::CanResetPassword> {
        None
    }
    fn as_password_reset_mut(&mut self) -> Option<&mut dyn super::CanResetPassword> {
        None
    }

    fn as_email_verification(&self) -> Option<&dyn super::MustVerifyEmail> {
        None
    }
}
