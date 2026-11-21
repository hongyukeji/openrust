#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use openrust::{
    auth::AuthIdentifier,
    contracts::auth::Authenticatable,
    database::eloquent::{Model, ModelMetadata, ModelState},
};

/// A distinct application model sharing the stock user's schema and auth fields.
pub(crate) struct User(crate::models::User);

impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        crate::models::User::metadata()
    }

    fn from_state(state: ModelState) -> Self {
        Self(crate::models::User::from_state(state))
    }

    fn state(&self) -> &ModelState {
        self.0.state()
    }

    fn state_mut(&mut self) -> &mut ModelState {
        self.0.state_mut()
    }
}

impl Authenticatable for User {
    fn getAuthIdentifierName(&self) -> &str {
        self.0.getAuthIdentifierName()
    }

    fn getAuthIdentifier(&self) -> AuthIdentifier {
        self.0.getAuthIdentifier()
    }

    fn getAuthPasswordName(&self) -> &str {
        self.0.getAuthPasswordName()
    }

    fn getAuthPassword(&self) -> Option<&str> {
        self.0.getAuthPassword()
    }

    fn getRememberToken(&self) -> Option<&str> {
        self.0.getRememberToken()
    }

    fn setRememberToken(&mut self, token: Option<String>) {
        self.0.setRememberToken(token);
    }

    fn getRememberTokenName(&self) -> &str {
        self.0.getRememberTokenName()
    }
}
