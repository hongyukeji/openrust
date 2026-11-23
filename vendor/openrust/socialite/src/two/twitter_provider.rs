use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User, abstract_provider::json,
    endpoints::mapped,
};
use crate::{Error, Result};
use openrust::http::client::Factory;
use serde_json::{Map, Value};
#[derive(Clone)]
pub struct TwitterProvider {
    endpoints: ProviderEndpoints,
}
impl Default for TwitterProvider {
    fn default() -> Self {
        Self::with_endpoints(ProviderEndpoints::new(
            "https://twitter.com/i/oauth2/authorize",
            "https://api.twitter.com/2/oauth2/token",
            "https://api.twitter.com/2/users/me",
        ))
    }
}
impl TwitterProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for TwitterProvider {
    fn get_auth_url(&self) -> &str {
        &self.endpoints.authorize
    }
    fn get_token_url(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["users.read", "users.email", "tweet.read"]
    }
    fn scope_separator(&self) -> &str {
        " "
    }
    fn uses_pkce(&self) -> bool {
        true
    }
    fn uses_rfc3986(&self) -> bool {
        true
    }
    fn token_basic_auth(&self) -> bool {
        true
    }
    fn refresh_token_includes_secret(&self) -> bool {
        false
    }
    fn customize_code_fields(
        &self,
        fields: &mut Map<String, Value>,
        _: &[String],
        _: bool,
        stateless: bool,
    ) {
        if stateless {
            fields.insert("state".into(), "state".into());
        }
    }
    fn get_user_by_token<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        _: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let response = json(
                http.pending_request()
                    .with_token(token)
                    .redact_payload()
                    .without_redirecting()
                    .with_query_parameters([(
                        "user.fields".into(),
                        "profile_image_url,confirmed_email".into(),
                    )])
                    .get(&self.endpoints.user)
                    .await?,
            )?;
            response
                .get("data")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or(Error::InvalidResponse)
        })
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        let fields = [
            ("id", user["id"].clone()),
            ("nickname", user["username"].clone()),
            ("name", user["name"].clone()),
            ("email", user["confirmed_email"].clone()),
            ("avatar", user["profile_image_url"].clone()),
        ];
        Ok(mapped(user, fields))
    }
}
