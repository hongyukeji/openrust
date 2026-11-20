use super::{
    ProviderConfig, ProviderDefinition, ProviderEndpoints, ProviderFuture, ProviderOptions, User,
    endpoints::{checked, identifier, mapped},
};
use crate::{Error, Result};
use openrust::http::client::Factory;
use serde_json::{Map, Value};

/// WeChat official-account web authorization. Website QR login uses WeixinWebProvider.
#[derive(Clone)]
pub struct WeixinProvider {
    endpoints: ProviderEndpoints,
    refresh: String,
    website: bool,
}
impl Default for WeixinProvider {
    fn default() -> Self {
        Self {
            endpoints: ProviderEndpoints::new(
                "https://open.weixin.qq.com/connect/oauth2/authorize#wechat_redirect",
                "https://api.weixin.qq.com/sns/oauth2/access_token",
                "https://api.weixin.qq.com/sns/userinfo",
            ),
            refresh: "https://api.weixin.qq.com/sns/oauth2/refresh_token".into(),
            website: false,
        }
    }
}
impl WeixinProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints, refresh: impl Into<String>) -> Self {
        Self {
            endpoints,
            refresh: refresh.into(),
            website: false,
        }
    }
    pub(super) fn website(mut self) -> Self {
        self.website = true;
        self
    }
    pub(super) fn website_default() -> Self {
        let mut provider = Self::default().website();
        provider.endpoints.authorize =
            "https://open.weixin.qq.com/connect/qrconnect#wechat_redirect".into();
        provider
    }
}
impl ProviderDefinition for WeixinProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        if self.website {
            &["snsapi_login"]
        } else {
            &["snsapi_userinfo"]
        }
    }
    fn customizeCodeFields(&self, fields: &mut Map<String, Value>, _: &[String], _: bool, _: bool) {
        if let Some(id) = fields.remove("client_id") {
            fields.insert("appid".into(), id);
        }
    }
    fn getAccessTokenResponse<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        fields: &'a Value,
    ) -> Option<ProviderFuture<'a, Value>> {
        Some(Box::pin(async move {
            let code = identifier(fields, "code")?;
            let response = checked(
                http.pendingRequest()
                    .redactPayload()
                    .withoutRedirecting()
                    .withQueryParameters([
                        ("appid".into(), config.client_id.clone()),
                        ("secret".into(), config.client_secret.to_string()),
                        ("code".into(), code.into()),
                        ("grant_type".into(), "authorization_code".into()),
                    ])
                    .get(&self.endpoints.token)
                    .await?,
            )?;
            identifier(&response, "access_token")?;
            identifier(&response, "openid")?;
            Ok(response)
        }))
    }
    fn configureTokenResponse(
        &self,
        response: &Value,
        options: &mut ProviderOptions,
    ) -> Result<()> {
        options.openId = Some(identifier(response, "openid")?.into());
        Ok(())
    }
    fn refreshTokenResponse<'a>(
        &'a self,
        http: &'a Factory,
        config: &'a ProviderConfig,
        refreshToken: &'a str,
    ) -> Option<ProviderFuture<'a, Value>> {
        Some(Box::pin(async move {
            let response = checked(
                http.pendingRequest()
                    .redactPayload()
                    .withoutRedirecting()
                    .withQueryParameters([
                        ("appid".into(), config.client_id.clone()),
                        ("grant_type".into(), "refresh_token".into()),
                        ("refresh_token".into(), refreshToken.into()),
                    ])
                    .get(&self.refresh)
                    .await?,
            )?;
            identifier(&response, "access_token")?;
            Ok(response)
        }))
    }
    fn getUserByToken<'a>(
        &'a self,
        _: &'a Factory,
        _: &'a str,
        _: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async { Err(Error::Configuration("openid")) })
    }
    fn getUserByTokenWithOptions<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
        _: &'a ProviderConfig,
        options: &'a ProviderOptions,
        _: &'a Map<String, Value>,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let id = options
                .openId
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or(Error::Configuration("openid"))?;
            if !self.website && scopes.iter().any(|s| s == "snsapi_base") {
                return Ok(serde_json::json!({"openid":id}));
            }
            let user = checked(
                http.pendingRequest()
                    .redactPayload()
                    .withoutRedirecting()
                    .withQueryParameters([
                        ("access_token".into(), token.into()),
                        ("openid".into(), id.into()),
                        ("lang".into(), "zh_CN".into()),
                    ])
                    .get(&self.endpoints.user)
                    .await?,
            )?;
            if identifier(&user, "openid")? != id {
                return Err(Error::InvalidResponse);
            }
            Ok(user)
        })
    }
    fn mapUserToObject(&self, user: Value) -> Result<User> {
        identifier(&user, "openid")?;
        let fields = [
            ("id", user["openid"].clone()),
            ("unionid", user["unionid"].clone()),
            ("nickname", user["nickname"].clone()),
            ("avatar", user["headimgurl"].clone()),
            ("name", Value::Null),
            ("email", Value::Null),
        ];
        Ok(mapped(user, fields))
    }
}
