use super::{ProviderDefinition, ProviderEndpoints, ProviderFuture, TwitterProvider, User};
use crate::Result;
use openrust::http::client::Factory;
use serde_json::{Map, Value};
#[derive(Clone)]
pub struct XProvider {
    inner: TwitterProvider,
}
impl Default for XProvider {
    fn default() -> Self {
        Self::with_endpoints(ProviderEndpoints::new(
            "https://x.com/i/oauth2/authorize",
            "https://api.x.com/2/oauth2/token",
            "https://api.x.com/2/users/me",
        ))
    }
}
impl XProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints) -> Self {
        Self {
            inner: TwitterProvider::with_endpoints(endpoints),
        }
    }
}
impl ProviderDefinition for XProvider {
    fn get_auth_url(&self) -> &str {
        self.inner.get_auth_url()
    }
    fn get_token_url(&self) -> &str {
        self.inner.get_token_url()
    }
    fn scopes(&self) -> &[&str] {
        self.inner.scopes()
    }
    fn scope_separator(&self) -> &str {
        self.inner.scope_separator()
    }
    fn uses_pkce(&self) -> bool {
        self.inner.uses_pkce()
    }
    fn uses_rfc3986(&self) -> bool {
        self.inner.uses_rfc3986()
    }
    fn token_basic_auth(&self) -> bool {
        self.inner.token_basic_auth()
    }
    fn refresh_token_includes_secret(&self) -> bool {
        self.inner.refresh_token_includes_secret()
    }
    fn customize_code_fields(
        &self,
        fields: &mut Map<String, Value>,
        scopes: &[String],
        bot: bool,
        stateless: bool,
    ) {
        self.inner
            .customize_code_fields(fields, scopes, bot, stateless);
    }
    fn get_user_by_token<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        self.inner.get_user_by_token(http, token, scopes)
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        self.inner.map_user_to_object(user)
    }
}
