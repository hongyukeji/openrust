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
        Self::withEndpoints(ProviderEndpoints::new(
            "https://x.com/i/oauth2/authorize",
            "https://api.x.com/2/oauth2/token",
            "https://api.x.com/2/users/me",
        ))
    }
}
impl XProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints) -> Self {
        Self {
            inner: TwitterProvider::withEndpoints(endpoints),
        }
    }
}
impl ProviderDefinition for XProvider {
    fn getAuthUrl(&self) -> &str {
        self.inner.getAuthUrl()
    }
    fn getTokenUrl(&self) -> &str {
        self.inner.getTokenUrl()
    }
    fn scopes(&self) -> &[&str] {
        self.inner.scopes()
    }
    fn scopeSeparator(&self) -> &str {
        self.inner.scopeSeparator()
    }
    fn usesPKCE(&self) -> bool {
        self.inner.usesPKCE()
    }
    fn usesRFC3986(&self) -> bool {
        self.inner.usesRFC3986()
    }
    fn tokenBasicAuth(&self) -> bool {
        self.inner.tokenBasicAuth()
    }
    fn refreshTokenIncludesSecret(&self) -> bool {
        self.inner.refreshTokenIncludesSecret()
    }
    fn customizeCodeFields(
        &self,
        fields: &mut Map<String, Value>,
        scopes: &[String],
        bot: bool,
        stateless: bool,
    ) {
        self.inner
            .customizeCodeFields(fields, scopes, bot, stateless);
    }
    fn getUserByToken<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        self.inner.getUserByToken(http, token, scopes)
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        self.inner.mapUserToObject(user)
    }
}
