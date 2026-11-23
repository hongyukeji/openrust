#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::auth::Authenticatable,
    database::{
        Value as DbValue,
        eloquent::{Model, ModelMetadata, ModelState},
    },
};
#[derive(Clone)]
pub struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("users")
            .primary_key("id")
            .incrementing(false)
            .timestamps(None, None);
        &META
    }
    fn from_state(s: ModelState) -> Self {
        Self(s)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
fn text<'a>(u: &'a User, key: &str) -> Option<&'a str> {
    match u.get_attribute(key) {
        Some(DbValue::Text(v)) => Some(v),
        _ => None,
    }
}
impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::try_from(self.get_key().unwrap()).unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        text(self, "password")
    }
    fn get_remember_token(&self) -> Option<&str> {
        text(self, "remember_token")
    }
    fn set_remember_token(&mut self, v: Option<String>) {
        self.set_attribute("remember_token", v.map_or(DbValue::Null, DbValue::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(&self, key: &str) -> AuthResult<Option<DbValue>> {
        self.get_attribute_value(key)
            .map_err(|e| AuthError::Backend(Box::new(e)))
    }
}
