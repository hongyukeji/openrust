use super::signature::{encode, sign};
use crate::{
    Error, Result,
    two::{ProviderConfig, Token, User},
};
use openrust::{
    contracts::session::Session as _,
    http::{Request, client::Factory},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct TwitterProvider {
    temporary: String,
    authorization: String,
    token: String,
    user: String,
}
impl Default for TwitterProvider {
    fn default() -> Self {
        Self::withEndpoints(
            "https://api.twitter.com/oauth/request_token",
            "https://api.twitter.com/oauth/authenticate",
            "https://api.twitter.com/oauth/access_token",
            "https://api.twitter.com/1.1/account/verify_credentials.json?include_email=true",
        )
    }
}
impl TwitterProvider {
    pub fn withEndpoints(
        temporary: impl Into<String>,
        authorization: impl Into<String>,
        token: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            temporary: temporary.into(),
            authorization: authorization.into(),
            token: token.into(),
            user: user.into(),
        }
    }
}
/// One request owns its scope, cached profile and borrowed Session.
pub struct AbstractProvider<'a> {
    request: &'a mut Request,
    config: ProviderConfig,
    http: Arc<Factory>,
    definition: TwitterProvider,
    scope: Option<String>,
    cached: Option<(Zeroizing<String>, Zeroizing<String>, Value)>,
}
impl<'a> AbstractProvider<'a> {
    pub fn new(
        request: &'a mut Request,
        config: ProviderConfig,
        http: Arc<Factory>,
        definition: TwitterProvider,
        scope: Option<String>,
    ) -> Self {
        Self {
            request,
            config,
            http,
            definition,
            scope,
            cached: None,
        }
    }
    pub fn scope(&mut self, scope: impl Into<String>) -> &mut Self {
        self.scope = Some(scope.into());
        self
    }
    pub fn redirectUrl(&mut self, url: impl Into<String>) -> &mut Self {
        self.config.redirect = url.into();
        self
    }
    pub async fn redirect(&mut self) -> Result<openrust::http::RedirectResponse> {
        let mut endpoint = url::Url::parse(&self.definition.temporary)
            .map_err(|_| Error::Configuration("OAuth1 endpoint"))?;
        if let Some(scope) = self.scope.as_deref().filter(|s| !s.is_empty() && *s != "0") {
            endpoint
                .query_pairs_mut()
                .append_pair("x_auth_access_type", scope);
        }
        let endpoint = endpoint.as_str();
        let mut protocol = self.protocol()?;
        protocol.insert("oauth_callback".into(), self.config.redirect.clone());
        let headers = self.headers("POST", endpoint, protocol, &BTreeMap::new(), "")?;
        let response = self
            .http
            .pending_request()
            .with_headers(headers)
            .redact_payload()
            .without_redirecting()
            .with_body(
                Vec::<u8>::new(),
                http::HeaderValue::from_static("application/x-www-form-urlencoded"),
            )
            .post(endpoint, &json!({}))
            .await?;
        let data = form(response)?;
        if data.get("oauth_callback_confirmed").map(String::as_str) != Some("true") {
            return Err(Error::InvalidResponse);
        }
        let (token, secret) = credentials(&data)?;
        self.request.session_mut()?.put(
            "oauth.temp",
            json!({"identifier":token.as_str(),"secret":secret.as_str()}),
        )?;
        let mut target = url::Url::parse(&self.definition.authorization)
            .map_err(|_| Error::Configuration("OAuth1 authorization URL"))?;
        target.query_pairs_mut().append_pair("oauth_token", &token);
        Ok(openrust::support::facades::Redirect::away(
            target.as_str(),
            Default::default(),
        )?)
    }
    pub async fn user(&mut self) -> Result<User> {
        // Request::has semantics distinguish absence from null/empty values.
        let token = self
            .request
            .input(Some("oauth_token"))
            .await?
            .ok_or(Error::MissingVerifier)?;
        let verifier = self
            .request
            .input(Some("oauth_verifier"))
            .await?
            .ok_or(Error::MissingVerifier)?;
        let temporary = self
            .request
            .session_mut()?
            .get("oauth.temp")
            .cloned()
            .ok_or(Error::MissingTemporaryCredentials)?;
        let stored = temporary
            .get("identifier")
            .and_then(Value::as_str)
            .ok_or(Error::MissingTemporaryCredentials)?;
        let secret = Zeroizing::new(
            temporary
                .get("secret")
                .and_then(Value::as_str)
                .ok_or(Error::MissingTemporaryCredentials)?
                .to_owned(),
        );
        if token.as_str() != Some(stored) {
            return Err(Error::InvalidTemporaryCredentials);
        }
        let verifier_body = match &verifier {
            Value::Null => None,
            Value::Bool(v) => Some(if *v { "1" } else { "0" }.to_owned()),
            Value::String(v) => Some(v.clone()),
            Value::Number(v) => Some(v.to_string()),
            _ => return Err(Error::MissingVerifier),
        };
        let verifier = match verifier {
            Value::String(v) => v,
            Value::Null => String::new(),
            Value::Number(v) => v.to_string(),
            Value::Bool(v) => if v { "1" } else { "" }.into(),
            _ => return Err(Error::MissingVerifier),
        };
        let endpoint = &self.definition.token;
        let mut protocol = self.protocol()?;
        protocol.insert("oauth_token".into(), stored.into());
        let body = BTreeMap::from([("oauth_verifier".into(), verifier)]);
        let headers = self.headers("POST", endpoint, protocol, &body, &secret)?;
        let payload = verifier_body
            .map(|v| BTreeMap::from([("oauth_verifier", v)]))
            .unwrap_or_default();
        let response = self
            .http
            .pending_request()
            .with_headers(headers)
            .redact_payload()
            .without_redirecting()
            .as_form()
            .post(endpoint, &payload)
            .await?;
        let (token, secret) = credentials(&form(response)?)?;
        self.profile(&token, &secret, true).await
    }
    pub async fn userFromTokenAndSecret(&mut self, token: &str, secret: &str) -> Result<User> {
        self.profile(token, secret, false).await
    }
    async fn profile(&mut self, token: &str, secret: &str, callback: bool) -> Result<User> {
        let raw = if let Some((old_token, old_secret, raw)) = &self.cached
            && old_token.as_str() == token
            && old_secret.as_str() == secret
        {
            raw.clone()
        } else {
            let endpoint = &self.definition.user;
            let mut protocol = self.protocol()?;
            protocol.insert("oauth_token".into(), token.into());
            let headers = self.headers("GET", endpoint, protocol, &BTreeMap::new(), secret)?;
            let response = self
                .http
                .pending_request()
                .with_headers(headers)
                .redact_payload()
                .without_redirecting()
                .get(endpoint)
                .await?;
            if !response.successful() {
                return Err(Error::ProviderStatus(response.status().as_u16()));
            }
            let raw: Value = response.json()?;
            if !raw.is_object() {
                return Err(Error::InvalidResponse);
            }
            self.cached = Some((
                Zeroizing::new(token.into()),
                Zeroizing::new(secret.into()),
                raw.clone(),
            ));
            raw
        };
        let mut user = mapUser(raw, callback)?;
        user.credentials = Some(Token {
            token: Zeroizing::new(token.into()),
            tokenSecret: Some(Zeroizing::new(secret.into())),
            refreshToken: None,
            expiresIn: None,
            approvedScopes: vec![],
        });
        Ok(user)
    }
    fn protocol(&self) -> Result<BTreeMap<String, String>> {
        let mut random = [0u8; 16];
        aws_lc_rs::rand::fill(&mut random).map_err(|_| Error::Random)?;
        let nonce = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Random)?
            .as_secs();
        Ok(BTreeMap::from([
            ("oauth_consumer_key".into(), self.config.client_id.clone()),
            ("oauth_nonce".into(), nonce),
            ("oauth_signature_method".into(), "HMAC-SHA1".into()),
            ("oauth_timestamp".into(), time.to_string()),
            ("oauth_version".into(), "1.0".into()),
        ]))
    }
    fn headers(
        &self,
        method: &str,
        endpoint: &str,
        mut protocol: BTreeMap<String, String>,
        body: &BTreeMap<String, String>,
        secret: &str,
    ) -> Result<http::HeaderMap> {
        let mut parameters = protocol.clone();
        parameters.extend(body.clone());
        protocol.insert(
            "oauth_signature".into(),
            sign(
                method,
                endpoint,
                &parameters,
                &self.config.client_secret,
                secret,
            )?,
        );
        let header = format!(
            "OAuth {}",
            protocol
                .into_iter()
                .map(|(k, v)| format!("{}=\"{}\"", encode(&k), encode(&v)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(http::HeaderMap::from_iter([(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&header)
                .map_err(|_| Error::Configuration("OAuth1 header"))?,
        )]))
    }
}
fn form(response: openrust::http::client::Response) -> Result<BTreeMap<String, String>> {
    if !response.successful() {
        return Err(Error::ProviderStatus(response.status().as_u16()));
    }
    let mut fields = BTreeMap::new();
    for (k, v) in url::form_urlencoded::parse(&response.body()?) {
        fields.insert(k.into_owned(), v.into_owned());
    }
    if fields.is_empty() || fields.contains_key("error") {
        return Err(Error::InvalidResponse);
    }
    Ok(fields)
}
fn credentials(data: &BTreeMap<String, String>) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
    Ok((
        Zeroizing::new(
            data.get("oauth_token")
                .ok_or(Error::InvalidResponse)?
                .clone(),
        ),
        Zeroizing::new(
            data.get("oauth_token_secret")
                .ok_or(Error::InvalidResponse)?
                .clone(),
        ),
    ))
}
/// Twitter's callback adds URL/location details; direct credentials retain League's extra only.
pub fn mapUser(raw: Value, callback: bool) -> Result<User> {
    let object = raw.as_object().ok_or(Error::InvalidResponse)?;
    let mut extra = object.clone();
    for key in [
        "id",
        "screen_name",
        "name",
        "location",
        "description",
        "profile_image_url",
        "email",
    ] {
        extra.remove(key);
    }
    for key in object.keys().filter(|k| k.contains("url")) {
        extra.remove(key);
    }
    if callback {
        for (key, value) in object.iter().filter(|(k, _)| k.contains("url")) {
            extra.insert(key.clone(), value.clone());
        }
        for key in ["location", "description"] {
            extra.insert(key.into(), raw.get(key).cloned().unwrap_or(Value::Null));
        }
    }
    let mut attributes = serde_json::Map::new();
    for (field, key) in [
        ("id", "id_str"),
        ("nickname", "screen_name"),
        ("name", "name"),
        ("email", "email"),
        ("avatar", "profile_image_url"),
    ] {
        attributes.insert(field.into(), raw.get(key).cloned().unwrap_or(Value::Null));
    }
    if callback {
        attributes.insert(
            "avatar_original".into(),
            raw.get("profile_image_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .replace("_normal", "")
                .into(),
        );
    }
    Ok(User::from_attributes(Value::Object(extra), attributes))
}
