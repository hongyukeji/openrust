use crate::{
    Authentication, Error, ProviderConfig, Result, Tokens,
    config::{ClientAuth, algorithm, endpoint},
    identity::{self, Expected},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, jwk::JwkSet};
use openrust::{
    cache::{Repository, Ttl, Value as CacheValue},
    contracts::{cache::Repository as _, session::Session as _},
    http::{
        Request,
        client::{Factory, PendingRequest},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;
#[derive(Deserialize)]
struct Metadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    userinfo_endpoint: Option<String>,
    response_types_supported: Vec<String>,
    subject_types_supported: Vec<String>,
    id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    code_challenge_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    end_session_endpoint: Option<String>,
}
pub struct Provider<'a> {
    name: String,
    request: &'a mut Request,
    config: ProviderConfig,
    http: Arc<Factory>,
    replay: Arc<Repository>,
    prompt: Option<String>,
    max_age: Option<u64>,
}
impl<'a> Provider<'a> {
    pub(crate) fn new(
        name: String,
        request: &'a mut Request,
        config: ProviderConfig,
        http: Arc<Factory>,
        replay: Arc<Repository>,
    ) -> Self {
        Self {
            name,
            request,
            config,
            http,
            replay,
            prompt: None,
            max_age: None,
        }
    }
    pub fn prompt(&mut self, value: impl Into<String>) -> &mut Self {
        self.prompt = Some(value.into());
        self
    }
    pub fn maxAge(&mut self, seconds: u64) -> &mut Self {
        self.max_age = Some(seconds);
        self
    }
    fn pending(&self) -> PendingRequest {
        self.http
            .pendingRequest()
            .acceptJson()
            .redactPayload()
            .withoutRedirecting()
            .timeout(self.config.timeout)
            .connectTimeout(self.config.timeout)
            .maxResponseBytes(1024 * 1024)
    }
    async fn metadata(&mut self) -> Result<Metadata> {
        let url = format!(
            "{}/.well-known/openid-configuration",
            self.config.issuer.trim_end_matches('/')
        );
        let metadata: Metadata = json_response(self.pending().get(&url).await?)?;
        if metadata.issuer != self.config.issuer
            || !metadata
                .response_types_supported
                .iter()
                .any(|s| s == "code")
            || !metadata
                .subject_types_supported
                .iter()
                .any(|s| matches!(s.as_str(), "public" | "pairwise"))
            || !metadata
                .id_token_signing_alg_values_supported
                .iter()
                .any(|a| {
                    self.config
                        .algorithms
                        .iter()
                        .any(|v| algorithm(a) == Some(*v))
                })
            || metadata
                .code_challenge_methods_supported
                .as_ref()
                .is_some_and(|m| !m.iter().any(|v| v == "S256"))
        {
            return Err(Error::InvalidDiscovery);
        }
        let methods = metadata
            .token_endpoint_auth_methods_supported
            .clone()
            .unwrap_or_else(|| vec!["client_secret_basic".into()]);
        if !methods.iter().any(|m| m == self.config.auth_method()) {
            return Err(Error::InvalidDiscovery);
        }
        for url in [
            &metadata.authorization_endpoint,
            &metadata.token_endpoint,
            &metadata.jwks_uri,
        ]
        .into_iter()
        .chain(metadata.userinfo_endpoint.iter())
        .chain(metadata.end_session_endpoint.iter())
        {
            let parsed = endpoint(url, self.config.allow_loopback_http)
                .map_err(|_| Error::InvalidDiscovery)?;
            if !self
                .config
                .allowed_origins
                .contains(&parsed.origin().ascii_serialization())
            {
                return Err(Error::InvalidDiscovery);
            }
        }
        let authorization = endpoint(
            &metadata.authorization_endpoint,
            self.config.allow_loopback_http,
        )?;
        if authorization.query_pairs().any(|(key, _)| {
            matches!(
                key.as_ref(),
                "client_id"
                    | "redirect_uri"
                    | "response_type"
                    | "scope"
                    | "state"
                    | "nonce"
                    | "code_challenge"
                    | "code_challenge_method"
                    | "prompt"
                    | "max_age"
            )
        }) {
            return Err(Error::InvalidDiscovery);
        }
        Ok(metadata)
    }
    fn session_key(&self) -> String {
        format!("oidc.attempts.{}", self.name)
    }
    pub async fn authorizationUrl(&mut self) -> Result<String> {
        let metadata = self.metadata().await?;
        let state = random(32)?;
        let nonce = random(32)?;
        let verifier = Zeroizing::new(random(64)?);
        let challenge = hash(&verifier);
        let mut url = endpoint(
            &metadata.authorization_endpoint,
            self.config.allow_loopback_http,
        )?;
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in [
                ("client_id", self.config.client_id.as_str()),
                ("redirect_uri", self.config.redirect.as_str()),
                ("response_type", "code"),
                ("scope", &self.config.scopes.join(" ")),
                ("state", &state),
                ("nonce", &nonce),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
            ] {
                q.append_pair(k, v);
            }
            if let Some(prompt) = &self.prompt {
                q.append_pair("prompt", prompt);
            }
            if let Some(age) = self.max_age {
                q.append_pair("max_age", &age.to_string());
            }
        }
        let attempt = json!({"state":state,"nonce":nonce,"verifier":verifier.as_str(),"issued_at":now()?,"config":self.config.fingerprint(),"max_age":self.max_age});
        let key = self.session_key();
        self.request.session_mut()?.put(&key, attempt)?;
        Ok(url.into())
    }
    pub async fn redirect(&mut self) -> Result<openrust::http::RedirectResponse> {
        let target = self.authorizationUrl().await?;
        Ok(openrust::support::facades::Redirect::away(
            &target,
            Default::default(),
        )?)
    }
    pub async fn user(&mut self) -> Result<Authentication> {
        let key = self.session_key();
        let attempt = self
            .request
            .session_mut()?
            .pull(&key)
            .ok_or(Error::InvalidState)?;
        let expected_state = attempt
            .get("state")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidState)?;
        let incoming = self
            .request
            .input(Some("state"))
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(Error::InvalidState)?;
        let fingerprint = self.config.fingerprint();
        let issued = attempt
            .get("issued_at")
            .and_then(Value::as_u64)
            .ok_or(Error::InvalidState)?;
        let now = now()?;
        if expected_state.is_empty()
            || !bool::from(expected_state.as_bytes().ct_eq(incoming.as_bytes()))
            || attempt.get("config").and_then(Value::as_str) != Some(fingerprint.as_str())
            || issued > now
            || now >= issued.saturating_add(self.config.attempt_ttl)
        {
            return Err(Error::InvalidState);
        }
        let key = format!("oidc:used:{}:{}", fingerprint, hash(expected_state));
        if !self
            .replay
            .add(
                &key,
                &CacheValue::Json(json!(true)),
                Some(Ttl::Seconds(self.config.attempt_ttl as i64)),
            )
            .await?
        {
            return Err(Error::InvalidState);
        }
        if self.request.input(Some("error")).await?.is_some() {
            return Err(Error::AuthorizationDenied);
        }
        if let Some(issuer) = self.request.input(Some("iss")).await?
            && issuer.as_str() != Some(self.config.issuer.as_str())
        {
            return Err(Error::InvalidState);
        }
        let code = Zeroizing::new(
            self.request
                .input(Some("code"))
                .await?
                .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
                .ok_or(Error::InvalidResponse)?,
        );
        let nonce = attempt
            .get("nonce")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(Error::InvalidState)?;
        let verifier = Zeroizing::new(
            attempt
                .get("verifier")
                .and_then(Value::as_str)
                .filter(|s| s.len() >= 43 && s.len() <= 128)
                .ok_or(Error::InvalidState)?
                .to_owned(),
        );
        let metadata = self.metadata().await?;
        let mut fields = json!({"grant_type":"authorization_code","code":code.as_str(),"redirect_uri":self.config.redirect,"code_verifier":verifier.as_str()});
        let mut pending = self.pending().asForm();
        match self.config.client_auth {
            ClientAuth::Basic => {
                pending = pending.withBasicAuth(
                    &form_encode(&self.config.client_id),
                    &form_encode(
                        self.config
                            .client_secret
                            .as_deref()
                            .map(|s| s.as_str())
                            .ok_or(Error::Configuration("client_secret"))?,
                    ),
                );
            }
            ClientAuth::Post => {
                fields["client_id"] = self.config.client_id.clone().into();
                fields["client_secret"] = self
                    .config
                    .client_secret
                    .as_deref()
                    .map(|s| s.as_str())
                    .ok_or(Error::Configuration("client_secret"))?
                    .into();
            }
            ClientAuth::None => {
                fields["client_id"] = self.config.client_id.clone().into();
            }
        }
        let token: Value = json_response(pending.post(&metadata.token_endpoint, &fields).await?)?;
        fn required(v: &Value, k: &str) -> Result<Zeroizing<String>> {
            v.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| Zeroizing::new(s.into()))
                .ok_or(Error::InvalidResponse)
        }
        if !token
            .get("token_type")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("Bearer"))
        {
            return Err(Error::InvalidResponse);
        }
        let tokens = Tokens {
            access_token: required(&token, "access_token")?,
            id_token: required(&token, "id_token")?,
            refresh_token: token
                .get("refresh_token")
                .map(|_| required(&token, "refresh_token"))
                .transpose()?,
            expires_in: match token.get("expires_in") {
                None => None,
                Some(v) => Some(v.as_u64().ok_or(Error::InvalidResponse)?),
            },
        };
        let jwks: JwkSet = json_response(self.pending().get(&metadata.jwks_uri).await?)?;
        if jwks.keys.is_empty() || jwks.keys.len() > 64 {
            return Err(Error::InvalidToken);
        }
        let algorithms = metadata
            .id_token_signing_alg_values_supported
            .iter()
            .filter_map(|v| algorithm(v))
            .collect::<Vec<Algorithm>>();
        let mut identity = identity::verify(
            &tokens.id_token,
            &jwks,
            Expected {
                config: &self.config,
                nonce,
                code: &code,
                access_token: &tokens.access_token,
                algorithms: &algorithms,
                max_age: attempt.get("max_age").and_then(Value::as_u64),
            },
        )?;
        if self.config.userinfo
            && let Some(endpoint) = metadata.userinfo_endpoint
        {
            let info: Value = json_response(
                self.pending()
                    .withToken(&tokens.access_token)
                    .get(&endpoint)
                    .await?,
            )?;
            if info.get("sub").and_then(Value::as_str) != Some(identity.subject.as_str()) {
                return Err(Error::InvalidUserInfo);
            }
            if let Some(email) = info.get("email") {
                identity.email = Some(email.as_str().ok_or(Error::InvalidUserInfo)?.to_owned());
                identity.email_verified =
                    info.get("email_verified").and_then(Value::as_bool) == Some(true);
            }
            identity.userinfo = Some(info);
        }
        Ok(Authentication { identity, tokens })
    }
}
fn form_encode(v: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("", v)
        .finish()
        .trim_start_matches('=')
        .to_owned()
}
fn json_response<T: serde::de::DeserializeOwned>(
    response: openrust::http::client::Response,
) -> Result<T> {
    if !response.successful() {
        return Err(Error::ProviderStatus(response.status().as_u16()));
    }
    response.json().map_err(|_| Error::InvalidResponse)
}
pub(crate) fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| Error::Random)
}
pub(crate) fn hash(value: &str) -> String {
    URL_SAFE_NO_PAD
        .encode(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, value.as_bytes()).as_ref())
}
fn random(bytes: usize) -> Result<String> {
    let mut buffer = Zeroizing::new(vec![0u8; bytes]);
    aws_lc_rs::rand::fill(&mut buffer).map_err(|_| Error::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(buffer.as_slice()))
}
