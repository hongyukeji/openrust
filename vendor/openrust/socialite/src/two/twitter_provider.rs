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
        Self::withEndpoints(ProviderEndpoints::new(
            "https://twitter.com/i/oauth2/authorize",
            "https://api.twitter.com/2/oauth2/token",
            "https://api.twitter.com/2/users/me",
        ))
    }
}
impl TwitterProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for TwitterProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["users.read", "users.email", "tweet.read"]
    }
    fn scopeSeparator(&self) -> &str {
        " "
    }
    fn usesPKCE(&self) -> bool {
        true
    }
    fn usesRFC3986(&self) -> bool {
        true
    }
    fn tokenBasicAuth(&self) -> bool {
        true
    }
    fn refreshTokenIncludesSecret(&self) -> bool {
        false
    }
    fn customizeCodeFields(
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
    fn getUserByToken<'a>(
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
    fn mapUserToObject(&self, user: Value) -> Result<User> {
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
