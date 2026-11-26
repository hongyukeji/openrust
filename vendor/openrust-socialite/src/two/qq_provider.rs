use super::{
    ProviderConfig, ProviderDefinition, ProviderEndpoints, ProviderFuture, ProviderOptions, User,
    endpoints::{checked, identifier, mapped},
};
use crate::{Error, Result};
use openrust::http::client::Factory;
use serde_json::{Map, Value};

/// QQ Connect website authorization. OpenID is resolved before requesting the profile.
#[derive(Clone)]
pub struct QqProvider {
    endpoints: ProviderEndpoints,
    me: String,
}
impl Default for QqProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://graph.qq.com/oauth2.0/authorize",
                "https://graph.qq.com/oauth2.0/token",
                "https://graph.qq.com/user/get_user_info",
            ),
            me: "https://graph.qq.com/oauth2.0/me".into(),
        }
    }
}
impl QqProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints, me: impl Into<String>) -> Self {
        Self {
            endpoints,
            me: me.into(),
        }
    }
    async fn profile(
        &self,
        http: &Factory,
        token: &str,
        config: &ProviderConfig,
        union: bool,
    ) -> Result<Value> {
        let mut params = vec![
            ("access_token".into(), token.into()),
            ("fmt".into(), "json".into()),
        ];
        if union {
            params.push(("unionid".into(), "1".into()));
        }
        let me = checked(
            http.pending_request()
                .redact_payload()
                .without_redirecting()
                .with_query_parameters(params)
                .get(&self.me)
                .await?,
        )?;
        let id = identifier(&me, "openid")?;
        if identifier(&me, "client_id")? != config.client_id {
            return Err(Error::InvalidResponse);
        }
        let profile = checked(
            http.pending_request()
                .redact_payload()
                .without_redirecting()
                .with_query_parameters([
                    ("access_token".into(), token.into()),
                    ("openid".into(), id.into()),
                    ("oauth_consumer_key".into(), config.client_id.clone()),
                ])
                .get(&self.endpoints.user)
                .await?,
        )?;
        // The PHP driver keeps these on a mutable provider. Keep them in this response instead.
        Ok(
            serde_json::json!({"profile":profile,"openid":id,"unionid":me.get("unionid").cloned().unwrap_or(Value::String(String::new()))}),
        )
    }
}
impl ProviderDefinition for QqProvider {
    fn get_auth_url(&self) -> &str {
        &self.endpoints.authorize
    }
    fn get_token_url(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["get_user_info"]
    }
    fn get_access_token_response<'a>(
        &'a self,
        http: &'a Factory,
        _: &'a ProviderConfig,
        fields: &'a Value,
    ) -> Option<ProviderFuture<'a, Value>> {
        Some(Box::pin(async move {
            let mut params: Vec<_> = fields
                .as_object()
                .ok_or(Error::InvalidResponse)?
                .iter()
                .map(|(k, v)| {
                    v.as_str()
                        .map(|v| (k.clone(), v.into()))
                        .ok_or(Error::InvalidResponse)
                })
                .collect::<Result<_>>()?;
            params.push(("fmt".into(), "json".into()));
            let response = checked(
                http.pending_request()
                    .redact_payload()
                    .without_redirecting()
                    .with_query_parameters(params)
                    .get(&self.endpoints.token)
                    .await?,
            )?;
            identifier(&response, "access_token")?;
            Ok(response)
        }))
    }
    fn refresh_token_response<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        token: &'a str,
    ) -> Option<ProviderFuture<'a, Value>> {
        Some(Box::pin(async move {
            let response = checked(
                http.pending_request()
                    .redact_payload()
                    .without_redirecting()
                    .with_query_parameters([
                        ("grant_type".into(), "refresh_token".into()),
                        ("client_id".into(), config.client_id.clone()),
                        ("client_secret".into(), config.client_secret.to_string()),
                        ("refresh_token".into(), token.into()),
                        ("fmt".into(), "json".into()),
                    ])
                    .get(&self.endpoints.token)
                    .await?,
            )?;
            identifier(&response, "access_token")?;
            Ok(response)
        }))
    }
    fn get_user_by_token<'a>(
        &'a self,
        _: &'a Factory,
        _: &'a str,
        _: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async { Err(Error::Configuration("client_id")) })
    }
    fn get_user_by_token_with_config<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        _: &'a [String],
        config: &'a ProviderConfig,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(self.profile(http, token, config, false))
    }
    fn get_user_by_token_with_options<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        _: &'a [String],
        config: &'a ProviderConfig,
        options: &'a ProviderOptions,
        _: &'a Map<String, Value>,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(self.profile(http, token, config, options.with_union_id))
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        let id = identifier(&user, "openid")?.to_owned();
        let profile = user
            .get("profile")
            .filter(|p| p.is_object())
            .ok_or(Error::InvalidResponse)?
            .clone();
        let fields = [
            ("id", id.into()),
            ("unionid", user["unionid"].clone()),
            ("nickname", profile["nickname"].clone()),
            ("name", Value::Null),
            ("email", Value::Null),
            ("avatar", profile["figureurl_qq_2"].clone()),
        ];
        Ok(mapped(profile, fields))
    }
}
