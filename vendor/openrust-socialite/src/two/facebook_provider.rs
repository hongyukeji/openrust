use super::{
    ProviderConfig, ProviderDefinition, ProviderEndpoints, ProviderFuture, ProviderOptions, User,
    abstract_provider::json,
    endpoints::{mapped, value},
    identity_token,
};
use crate::{Error, Result};
use openrust::http::client::Factory;
use serde_json::{Map, Value};
use subtle::ConstantTimeEq as _;
#[derive(Clone)]
pub struct FacebookProvider {
    endpoints: ProviderEndpoints,
    jwks: String,
}
impl Default for FacebookProvider {
    fn default() -> Self {
        Self::with_endpoints(
            ProviderEndpoints::new(
                "https://www.facebook.com/v23.0/dialog/oauth",
                "https://graph.facebook.com/v23.0/oauth/access_token",
                "https://graph.facebook.com/v23.0/me",
            ),
            "https://limited.facebook.com/.well-known/oauth/openid/jwks/",
        )
    }
}
impl FacebookProvider {
    pub fn with_endpoints(endpoints: ProviderEndpoints, jwks: impl Into<String>) -> Self {
        Self {
            endpoints,
            jwks: jwks.into(),
        }
    }
    fn versioned(url: &str, options: &ProviderOptions) -> String {
        match &options.graph_version {
            Some(version) => url.replacen("/v23.0/", &format!("/{version}/"), 1),
            None => url.into(),
        }
    }
    fn nonce(value: &Value) -> Option<String> {
        match value {
            Value::String(v) => Some(v.clone()),
            Value::Number(v) => Some(v.to_string()),
            Value::Bool(v) => Some(if *v { "1" } else { "" }.into()),
            _ => None,
        }
    }
    fn has_kid(token: &str) -> bool {
        use base64::Engine as _;
        let head = token.split('.').next().unwrap_or("");
        [
            base64::engine::general_purpose::URL_SAFE_NO_PAD,
            base64::engine::general_purpose::URL_SAFE,
            base64::engine::general_purpose::STANDARD,
        ]
        .into_iter()
        .filter_map(|engine| engine.decode(head).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .any(|header| header.get("kid").is_some_and(|v| !v.is_null()))
    }
}
impl ProviderDefinition for FacebookProvider {
    fn get_auth_url(&self) -> &str {
        &self.endpoints.authorize
    }
    fn get_token_url(&self) -> &str {
        &self.endpoints.token
    }
    fn scopes(&self) -> &[&str] {
        &["email"]
    }
    fn authorization_endpoint(&self, options: &ProviderOptions) -> String {
        Self::versioned(&self.endpoints.authorize, options)
    }
    fn token_endpoint(&self, options: &ProviderOptions) -> String {
        Self::versioned(&self.endpoints.token, options)
    }
    fn token_accepts_json(&self) -> bool {
        false
    }
    fn customize_code_fields_with_options(
        &self,
        fields: &mut Map<String, Value>,
        _: &[String],
        _: bool,
        _: bool,
        options: &ProviderOptions,
    ) {
        if options.popup {
            fields.insert("display".into(), "popup".into());
        }
        if options.re_request {
            fields.insert("auth_type".into(), "rerequest".into());
        }
    }
    fn normalize_token_response(&self, mut response: Value, _: bool) -> Result<Value> {
        let object = response.as_object_mut().ok_or(Error::InvalidResponse)?;
        // PHP evaluates the Arr::add first argument before Arr::pull mutates the local array.
        let expires = object.get("expires").cloned().unwrap_or(Value::Null);
        if object.get("expires_in").is_none_or(Value::is_null) {
            object.insert("expires_in".into(), expires);
        }
        Ok(response)
    }
    fn get_user_by_token<'a>(
        &'a self,
        _: &'a Factory,
        _: &'a str,
        _: &'a [String],
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async { Err(Error::Configuration("client_secret")) })
    }
    fn get_user_by_token_with_options<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        _: &'a [String],
        config: &'a ProviderConfig,
        options: &'a ProviderOptions,
        parameters: &'a Map<String, Value>,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            if Self::has_kid(token) {
                let mut claims = identity_token::verify(
                    http,
                    &self.jwks,
                    token,
                    identity_token::Requirements {
                        issuer: "https://www.facebook.com",
                        audience: &config.client_id,
                        algorithm: Some(jsonwebtoken::Algorithm::RS256),
                        first_key: true,
                    },
                )
                .await?;
                let expected = options
                    .expected_nonce
                    .clone()
                    .or_else(|| {
                        parameters
                            .get("nonce")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .ok_or(Error::InvalidIdentityToken)?;
                let nonce = claims
                    .get("nonce")
                    .and_then(Self::nonce)
                    .ok_or(Error::InvalidIdentityToken)?;
                if !bool::from(expected.as_bytes().ct_eq(nonce.as_bytes())) {
                    return Err(Error::InvalidIdentityToken);
                }
                let sub = claims
                    .get("sub")
                    .cloned()
                    .ok_or(Error::InvalidIdentityToken)?;
                claims["id"] = sub;
                for (source, target) in [("given_name", "first_name"), ("family_name", "last_name")]
                {
                    if let Some(v) = claims.get(source).filter(|v| !v.is_null()).cloned() {
                        claims[target] = v;
                    }
                }
                return Ok(claims);
            }
            let fields = options.fields.clone().unwrap_or_else(|| {
                [
                    "name",
                    "email",
                    "gender",
                    "verified",
                    "link",
                    "picture.width(1920)",
                ]
                .map(str::to_owned)
                .to_vec()
            });
            let mut params = vec![
                ("access_token".into(), token.into()),
                ("fields".into(), fields.join(",")),
            ];
            if !config.client_secret.is_empty() && config.client_secret.as_str() != "0" {
                let key = aws_lc_rs::hmac::Key::new(
                    aws_lc_rs::hmac::HMAC_SHA256,
                    config.client_secret.as_bytes(),
                );
                let signature = aws_lc_rs::hmac::sign(&key, token.as_bytes());
                params.push((
                    "appsecret_proof".into(),
                    signature
                        .as_ref()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                ));
            }
            json(
                http.pending_request()
                    .accept_json()
                    .redact_payload()
                    .without_redirecting()
                    .with_query_parameters(params)
                    .get(&Self::versioned(&self.endpoints.user, options))
                    .await?,
            )
        })
    }
    fn map_user_to_object(&self, user: Value) -> Result<User> {
        let avatar = value(&user, &["picture", "data", "url"]);
        let fields = [
            ("id", user["id"].clone()),
            ("nickname", Value::Null),
            ("name", user["name"].clone()),
            ("email", user["email"].clone()),
            ("avatar", avatar.clone()),
            ("avatar_original", avatar),
            ("profileUrl", user["link"].clone()),
        ];
        Ok(mapped(user, fields))
    }
}
