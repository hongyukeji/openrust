use super::{
    ProviderConfig, ProviderDefinition, ProviderEndpoints, ProviderFuture, User,
    abstract_provider::json, endpoints::mapped, identity_token,
};
use crate::{Error, Result};
use openrust::http::client::Factory;
use serde_json::Value;
#[derive(Clone)]
pub struct GoogleProvider {
    endpoints: ProviderEndpoints,
    jwks: String,
}
impl Default for GoogleProvider {
    fn default() -> Self {
        Self::withEndpoints(
            ProviderEndpoints::new(
                "https://accounts.google.com/o/oauth2/auth",
                "https://www.googleapis.com/oauth2/v4/token",
                "https://www.googleapis.com/oauth2/v3/userinfo",
            ),
            "https://www.googleapis.com/oauth2/v3/certs",
        )
    }
}
impl GoogleProvider {
    pub fn withEndpoints(endpoints: ProviderEndpoints, jwks: impl Into<String>) -> Self {
        Self {
            endpoints,
            jwks: jwks.into(),
        }
    }
    fn isJwtToken(token: &str) -> bool {
        token.len() > 100 && token.bytes().filter(|b| *b == b'.').count() == 2
    }
}
impl ProviderDefinition for GoogleProvider {
    fn getAuthUrl(&self) -> &str {
        &self.endpoints.authorize
    }
    fn getTokenUrl(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["openid", "profile", "email"]
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
            if Self::isJwtToken(token) {
                return Err(Error::Configuration("client_id"));
            }
            json(
                http.pendingRequest()
                    .acceptJson()
                    .withToken(token)
                    .redactPayload()
                    .withoutRedirecting()
                    .withQueryParameters([("prettyPrint".into(), "false".into())])
                    .get(&self.endpoints.user)
                    .await?,
            )
        })
    }
    fn getUserByTokenWithConfig<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
        config: &'a ProviderConfig,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            if Self::isJwtToken(token) {
                identity_token::verify(
                    http,
                    &self.jwks,
                    token,
                    "https://accounts.google.com",
                    &config.client_id,
                )
                .await
            } else {
                self.getUserByToken(http, token, scopes).await
            }
        })
    }
    fn normalizeRefreshTokenResponse(
        &self,
        mut response: Value,
        refreshToken: &str,
    ) -> Result<Value> {
        if let Some(object) = response.as_object_mut() {
            if !object.contains_key("refresh_token") {
                object.insert("refresh_token".into(), refreshToken.into());
            }
        } else {
            return Err(Error::InvalidResponse);
        }
        Ok(response)
    }
    fn mapUserToObject(&self, mut user: Value) -> Result<User> {
        let id = user["sub"].clone();
        let verified = user["email_verified"].clone();
        let link = user["profile"].clone();
        let object = user.as_object_mut().ok_or(Error::InvalidResponse)?;
        object.insert("id".into(), id.clone());
        object.insert("verified_email".into(), verified);
        object.insert("link".into(), link);
        let fields = [
            ("id", id),
            ("nickname", user["nickname"].clone()),
            ("name", user["name"].clone()),
            ("email", user["email"].clone()),
            ("avatar", user["picture"].clone()),
            ("avatar_original", user["picture"].clone()),
        ];
        Ok(mapped(user, fields))
    }
}
