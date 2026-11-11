use openrust::{
    auth::AuthIdentifier,
    contracts::auth::Authenticatable,
    database::{
        Value,
        eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
    },
};

pub(crate) struct User(ModelState);

impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("users")
            .fillable(&["name", "email", "password"])
            .hidden(&["password", "remember_token"]);
        &METADATA
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("password", AttributeCast::Hashed)]
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        self.getKeyName()
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.getKey()
            .and_then(|value| value.try_into().ok())
            .unwrap_or_else(|| AuthIdentifier::String(String::new()))
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        text(self.getAttribute("password"))
    }
    fn get_remember_token(&self) -> Option<&str> {
        text(self.getAttribute("remember_token"))
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.setAttribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}
