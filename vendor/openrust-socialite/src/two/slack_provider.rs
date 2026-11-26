use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User,
    abstract_provider::json,
    endpoints::{mapped, value},
};
use crate::{Error, Result};
use openrust::http::client::Factory;
use serde_json::{Map, Value};
#[derive(Clone)]
pub struct SlackProvider {
    endpoints: ProviderEndpoints,
}
impl Default for SlackProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://slack.com/oauth/v2/authorize",
                "https://slack.com/api/oauth.v2.access",
                "https://slack.com/api/users.identity",
            ),
        }
    }
}
impl SlackProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
}
impl ProviderDefinition for SlackProvider {
    fn get_auth_url(&self) -> &str {
        &self.endpoints.authorize
    }
    fn get_token_url(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &[
            "identity.basic",
            "identity.email",
            "identity.team",
            "identity.avatar",
        ]
    }
    fn customize_code_fields(
        &self,
        fields: &mut Map<String, Value>,
        scopes: &[String],
        bot_user: bool,
        _stateless: bool,
    ) {
        if !bot_user {
            fields.insert("scope".into(), "".into());
            fields.insert("user_scope".into(), scopes.join(",").into());
        }
    }
    fn normalize_token_response(&self, response: Value, bot_user: bool) -> Result<Value> {
        if bot_user {
            Ok(response)
        } else {
            response
                .get("authed_user")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or(Error::InvalidResponse)
        }
    }
    fn get_user_by_token<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        _: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            json(
                http.pending_request()
                    .with_token(token)
                    .redact_payload()
                    .without_redirecting()
                    .get(&self.endpoints.user)
                    .await?,
            )
        })
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        let fields = [
            ("id", value(&user, &["user", "id"])),
            ("name", value(&user, &["user", "name"])),
            ("email", value(&user, &["user", "email"])),
            ("avatar", value(&user, &["user", "image_512"])),
            ("organization_id", value(&user, &["team", "id"])),
        ];
        Ok(mapped(user, fields))
    }
}
