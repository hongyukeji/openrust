#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

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
            .primaryKey("id")
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
    match u.getAttribute(key) {
        Some(DbValue::Text(v)) => Some(v),
        _ => None,
    }
}
impl Authenticatable for User {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        AuthIdentifier::try_from(self.getKey().unwrap()).unwrap()
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        text(self, "password")
    }
    fn getRememberToken(&self) -> Option<&str> {
        text(self, "remember_token")
    }
    fn setRememberToken(&mut self, v: Option<String>) {
        self.setAttribute("remember_token", v.map_or(DbValue::Null, DbValue::Text));
    }
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
    fn getAuthAttribute(&self, key: &str) -> AuthResult<Option<DbValue>> {
        self.getAttributeValue(key)
            .map_err(|e| AuthError::Backend(Box::new(e)))
    }
}
