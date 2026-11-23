use super::WeixinProvider;
use crate::Result;
use crate::two::{
    ProviderConfig, ProviderDefinition, ProviderEndpoints, ProviderFuture, ProviderOptions, User,
};
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
    pub fn with_endpoints(endpoints: ProviderEndpoints, refresh: impl Into<String>) -> Self {
        Self(WeixinProvider::with_endpoints(endpoints, refresh).website())
    }
}
impl ProviderDefinition for WeixinWebProvider {
    fn get_auth_url(&self) -> &str {
        self.0.get_auth_url()
    }
    fn get_token_url(&self) -> &str {
        self.0.get_token_url()
    }
    fn scopes(&self) -> &[&str] {
        self.0.scopes()
    }
    fn customize_code_fields(
        &self,
        fields: &mut Map<String, Value>,
        scopes: &[String],
        bot: bool,
        stateless: bool,
    ) {
        self.0.customize_code_fields(fields, scopes, bot, stateless);
    }
    fn get_access_token_response<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        fields: &'a Value,
    ) -> Option<ProviderFuture<'a, Value>> {
        self.0.get_access_token_response(http, config, fields)
    }
    fn configure_token_response(
        &self,
        response: &Value,
        options: &mut ProviderOptions,
    ) -> Result<()> {
        self.0.configure_token_response(response, options)
    }
    fn refresh_token_response<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        token: &'a str,
    ) -> Option<ProviderFuture<'a, Value>> {
        self.0.refresh_token_response(http, config, token)
    }
    fn get_user_by_token<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        self.0.get_user_by_token(http, token, scopes)
    }
    fn get_user_by_token_with_options<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
        config: &'a ProviderConfig,
        options: &'a ProviderOptions,
        parameters: &'a Map<String, Value>,
    ) -> ProviderFuture<'a, Value> {
        self.0
            .get_user_by_token_with_options(http, token, scopes, config, options, parameters)
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        self.0.map_user_to_object(user)
    }
}
