#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

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
    fn get_auth_identifier_name(&self) -> &str {
        self.0.get_auth_identifier_name()
    }

    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.0.get_auth_identifier()
    }

    fn get_auth_password_name(&self) -> &str {
        self.0.get_auth_password_name()
    }

    fn get_auth_password(&self) -> Option<&str> {
        self.0.get_auth_password()
    }

    fn get_remember_token(&self) -> Option<&str> {
        self.0.get_remember_token()
    }

    fn set_remember_token(&mut self, token: Option<String>) {
        self.0.set_remember_token(token);
    }

    fn get_remember_token_name(&self) -> &str {
        self.0.get_remember_token_name()
    }
}
