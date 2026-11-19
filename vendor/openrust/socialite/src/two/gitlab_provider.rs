use super::{
    ProviderDefinition, ProviderEndpoints, ProviderFuture, User, abstract_provider::json,
    endpoints::mapped,
};
use crate::Result;
use openrust::http::client::Factory;
use serde_json::Value;
#[derive(Clone)]
pub struct GitlabProvider {
    endpoints: ProviderEndpoints,
}
impl Default for GitlabProvider {
    fn default() -> Self {
        let mut p = Self {
            endpoints: ProviderEndpoints::new("", "", ""),
        };
        p.setHost("https://gitlab.com");
        p
    }
}
impl GitlabProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
    pub fn setHost(&mut self, host: &str) -> &mut Self {
        if !host.is_empty() && host != "0" {
            let host = host.trim_end_matches('/');
            self.endpoints = ProviderEndpoints::new(
                format!("{host}/oauth/authorize"),
                format!("{host}/oauth/token"),
                format!("{host}/api/v3/user"),
            );
        }
        self
    }
}
impl ProviderDefinition for GitlabProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["read_user"]
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
                    .redactPayload()
                    .withoutRedirecting()
                    .withQueryParameters([("access_token".into(), token.into())])
                    .get(&self.endpoints.user)
                    .await?,
            )
        })
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        let fields = [
            ("id", user["id"].clone()),
            ("nickname", user["username"].clone()),
            ("name", user["name"].clone()),
            ("email", user["email"].clone()),
            ("avatar", user["avatar_url"].clone()),
        ];
        Ok(mapped(user, fields))
    }
}
