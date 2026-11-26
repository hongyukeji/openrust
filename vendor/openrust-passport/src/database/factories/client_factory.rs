//! Fixed Passport 13.10.0 ClientFactory states, backed by Eloquent factories.
use crate::{Client, ClientModel};
use openrust::{
    database::{
        DatabaseResult, Value,
        eloquent::{Attributes, ModelState, factories::Factory},
    },
    support::Str,
};

/// A compiled client model binding; custom clients use `ClientFactory::<M>::new()`.
/// Construction can fail when the current application's connection config is invalid.
pub struct ClientFactory<M: ClientModel = Client>(Factory<M>);

impl<M: ClientModel> std::ops::Deref for ClientFactory<M> {
    type Target = Factory<M>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M: ClientModel> ClientFactory<M> {
    pub fn new() -> DatabaseResult<Self> {
        let model = M::from_state(ModelState::default());
        let connection = model.resolve_connection_name()?;
        Ok(Self(
            Factory::try_with_faker(|faker| {
                Ok(Attributes::from([
                    ("owner_id".into(), Value::Null),
                    ("owner_type".into(), Value::Null),
                    ("name".into(), faker.company().into()),
                    ("secret".into(), Str::random(40).into()),
                    (
                        "redirect_uris".into(),
                        Value::Json(serde_json::json!([faker.url()?])),
                    ),
                    (
                        "grant_types".into(),
                        Value::Json(serde_json::json!(["authorization_code", "refresh_token"])),
                    ),
                    ("revoked".into(), false.into()),
                ]))
            })
            .connection(connection.as_deref()),
        ))
    }

    /// Returns the existing Eloquent builder for counts, relationships and callbacks.
    pub fn into_factory(self) -> Factory<M> {
        self.0
    }

    #[must_use]
    pub fn state(self, state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static) -> Self {
        Self(self.0.state(state))
    }

    #[must_use]
    pub fn as_public(self) -> Self {
        self.state(|_| Attributes::from([("secret".into(), Value::Null)]))
    }
    fn grants(self, grants: &'static [&'static str], clear_redirects: bool) -> Self {
        self.state(move |_| {
            let mut attributes =
                Attributes::from([("grant_types".into(), Value::Json(serde_json::json!(grants)))]);
            if clear_redirects {
                attributes.insert("redirect_uris".into(), Value::Json(serde_json::json!([])));
            }
            attributes
        })
    }
    #[must_use]
    pub fn as_password_client(self) -> Self {
        self.grants(&["password", "refresh_token"], true)
    }
    #[must_use]
    pub fn as_personal_access_token_client(self) -> Self {
        self.grants(&["personal_access"], true)
    }
    #[must_use]
    pub fn as_implicit_client(self) -> Self {
        self.grants(&["implicit"], false).as_public()
    }
    #[must_use]
    pub fn as_client_credentials(self) -> Self {
        self.grants(&["client_credentials"], true)
    }
    #[must_use]
    pub fn as_device_code_client(self) -> Self {
        self.grants(
            &[
                "urn:ietf:params:oauth:grant-type:device_code",
                "refresh_token",
            ],
            true,
        )
    }
}
