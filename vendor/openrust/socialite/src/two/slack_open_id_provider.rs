use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User, abstract_provider::json,
    endpoints::mapped,
};
use crate::Result;
use openrust::http::client::Factory;
use serde_json::Value;
#[derive(Clone)]
pub struct SlackOpenIdProvider {
    endpoints: ProviderEndpoints,
}
impl Default for SlackOpenIdProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://slack.com/openid/connect/authorize",
                "https://slack.com/api/openid.connect.token",
                "https://slack.com/api/openid.connect.userInfo",
            ),
        }
    }
}
impl SlackOpenIdProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for SlackOpenIdProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["openid", "email", "profile"]
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
                http.pendingRequest()
                    .withToken(token)
                    .redactPayload()
                    .withoutRedirecting()
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
            ("email", user["email"].clone()),
            ("avatar", user["picture"].clone()),
            ("organization_id", user["https://slack.com/team_id"].clone()),
        ];
        Ok(mapped(user, fields))
    }
}
