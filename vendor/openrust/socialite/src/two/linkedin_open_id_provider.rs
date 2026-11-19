use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User, abstract_provider::json,
    endpoints::mapped, linkedin_provider::profile_request,
};
use crate::Result;
use openrust::http::client::Factory;
use serde_json::Value;
#[derive(Clone)]
pub struct LinkedInOpenIdProvider {
    endpoints: ProviderEndpoints,
}
impl Default for LinkedInOpenIdProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://www.linkedin.com/oauth/v2/authorization",
                "https://www.linkedin.com/oauth/v2/accessToken",
                "https://api.linkedin.com/v2/userinfo",
            ),
        }
    }
}
impl LinkedInOpenIdProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for LinkedInOpenIdProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["openid", "profile", "email"]
    }
    fn scopeSeparator(&self) -> &str {
        " "
    }
    fn getUserByToken<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        _: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            json(
                profile_request(http, token)
                    .withQueryParameters([(
                        "projection".into(),
                        "(sub,email,email_verified,name,given_name,family_name,picture)".into(),
                    )])
                    .get(&self.endpoints.user)
                    .await?,
            )
        })
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        let fields = [
            ("id", user["sub"].clone()),
            ("nickname", Value::Null),
            ("name", user["name"].clone()),
            ("first_name", user["given_name"].clone()),
            ("last_name", user["family_name"].clone()),
            ("email", user["email"].clone()),
            ("email_verified", user["email_verified"].clone()),
            ("avatar", user["picture"].clone()),
            ("avatar_original", user["picture"].clone()),
        ];
        Ok(mapped(user, fields))
    }
}
