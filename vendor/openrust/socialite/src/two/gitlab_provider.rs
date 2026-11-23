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
        p.set_host("https://gitlab.com");
        p
    }
}
impl GitlabProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints) -> Self {
        Self { endpoints }
    }
    pub fn set_host(&mut self, host: &str) -> &mut Self {
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
    fn get_auth_url(&self) -> &str {
        &self.endpoints.authorize
    }
    fn get_token_url(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["read_user"]
    }
    fn scope_separator(&self) -> &str {
        " "
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
                    .redact_payload()
                    .without_redirecting()
                    .with_query_parameters([("access_token".into(), token.into())])
                    .get(&self.endpoints.user)
                    .await?,
            )
        })
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
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
