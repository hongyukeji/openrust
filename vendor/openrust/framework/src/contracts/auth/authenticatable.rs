use std::any::Any;

use crate::auth::AuthIdentifier;

/// Identity and credential fields exposed by an authenticatable user.
pub trait Authenticatable: Any + Send + Sync {
    fn get_auth_identifier_name(&self) -> &str;
    fn get_auth_identifier(&self) -> AuthIdentifier;
    fn get_auth_password_name(&self) -> &str;
    fn get_auth_password(&self) -> Option<&str>;
    fn get_remember_token(&self) -> Option<&str>;
    fn set_remember_token(&mut self, token: Option<String>);
    fn get_remember_token_name(&self) -> &str;
}
