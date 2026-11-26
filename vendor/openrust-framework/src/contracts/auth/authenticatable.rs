use std::any::Any;

use crate::{
    auth::{AuthIdentifier, AuthResult},
    database::Value,
};

/// Identity and credential fields exposed by an authenticatable user.
pub trait Authenticatable: Any + Send + Sync {
    fn get_auth_identifier_name(&self) -> &str;
    fn get_auth_identifier(&self) -> AuthIdentifier;
    fn get_auth_password_name(&self) -> &str;
    fn get_auth_password(&self) -> Option<&str>;
    fn get_remember_token(&self) -> Option<&str>;
    fn set_remember_token(&mut self, token: Option<String>);
    fn get_remember_token_name(&self) -> &str;

    /// Reads a named attribute used by authentication-aware middleware.
    fn get_auth_attribute(&self, _name: &str) -> AuthResult<Option<Value>> {
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
