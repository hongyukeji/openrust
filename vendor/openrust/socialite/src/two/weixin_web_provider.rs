use super::{
    ProviderConfig, ProviderDefinition, ProviderEndpoints, ProviderFuture, ProviderOptions, User,
    WeixinProvider,
};
use crate::Result;
use openrust::http::client::Factory;
use serde_json::{Map, Value};

/// WeChat Open Platform website QR login, with request-owned identity context.
#[derive(Clone)]
pub struct WeixinWebProvider(WeixinProvider);
impl Default for WeixinWebProvider {
    fn default() -> Self {
        Self(WeixinProvider::website_default())
    }
}
impl WeixinWebProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints, refresh: impl Into<String>) -> Self {
        Self(WeixinProvider::withEndpoints(endpoints, refresh).website())
    }
}
impl ProviderDefinition for WeixinWebProvider {
    fn getAuthUrl(&self) -> &str {
        self.0.getAuthUrl()
    }
    fn getTokenUrl(&self) -> &str {
        self.0.getTokenUrl()
    }
    fn scopes(&self) -> &[&str] {
        self.0.scopes()
    }
    fn customizeCodeFields(
        &self,
        fields: &mut Map<String, Value>,
        scopes: &[String],
        bot: bool,
        stateless: bool,
    ) {
        self.0.customizeCodeFields(fields, scopes, bot, stateless);
    }
    fn getAccessTokenResponse<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        fields: &'a Value,
    ) -> Option<ProviderFuture<'a, Value>> {
        self.0.getAccessTokenResponse(http, config, fields)
    }
    fn configureTokenResponse(
        &self,
        response: &Value,
        options: &mut ProviderOptions,
    ) -> Result<()> {
        self.0.configureTokenResponse(response, options)
    }
    fn refreshTokenResponse<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        token: &'a str,
    ) -> Option<ProviderFuture<'a, Value>> {
        self.0.refreshTokenResponse(http, config, token)
    }
    fn getUserByToken<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        self.0.getUserByToken(http, token, scopes)
    }
    fn getUserByTokenWithOptions<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
        config: &'a ProviderConfig,
        options: &'a ProviderOptions,
        parameters: &'a Map<String, Value>,
    ) -> ProviderFuture<'a, Value> {
        self.0
            .getUserByTokenWithOptions(http, token, scopes, config, options, parameters)
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        self.0.mapUserToObject(user)
    }
}
