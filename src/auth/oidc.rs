//! 基于授权码与 PKCE 的 `OpenID Connect` relying-party 客户端。

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, RwLock},
};
use std::{net::IpAddr, time::Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::{Request, Response};
use jsonwebtoken::jwk::{Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenType,
};
use oauth2::{
    AuthUrl, AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, ExtraTokenFields, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    StandardRevocableToken, StandardTokenResponse, TokenResponse, TokenUrl,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::audit::{Audit, AuditEvent, AuditOutcome, AuditPolicy, AuditSubject, DatabaseAuditSink};
use crate::auth::{AuthFlows, LoginAttempt};
use crate::core::{BoxFuture, Error, Result};
use crate::db::{DatabaseTransaction, Db};
use crate::http::{DependencyClient, HttpBody};
use crate::security::Encryptor;

/// 通过 ID Token 验证的外部身份；仅包含稳定 subject 与可选邮箱声明。
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedExternalIdentity {
    issuer: String,
    subject: String,
    email: Option<String>,
    email_verified: bool,
}

impl VerifiedExternalIdentity {
    /// 为满足 OpenRust 身份验证 contract 的自定义 provider 创建已验证主体。
    ///
    /// 实现方必须先完成其协议所需的签名、issuer、audience、时效与抗重放校验。
    pub fn new(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        email: Option<String>,
        email_verified: bool,
    ) -> Result<Self> {
        let issuer = issuer.into();
        let subject = subject.into();
        if issuer.trim().is_empty()
            || issuer.len() > 512
            || issuer.bytes().any(|byte| byte.is_ascii_control())
            || subject.trim().is_empty()
            || subject.len() > 512
            || subject.bytes().any(|byte| byte.is_ascii_control())
            || email.as_ref().is_some_and(|value| value.trim().is_empty())
            || email.as_ref().is_some_and(|value| value.len() > 320)
            || (email.is_none() && email_verified)
        {
            return Err(oidc_error());
        }
        Ok(Self {
            issuer,
            subject,
            email,
            email_verified,
        })
    }

    pub(crate) fn identity_hash(&self, provider: &str) -> String {
        external_identity_hash(provider, &self.issuer, &self.subject)
    }

    /// 返回经过签名与 issuer 校验的 provider issuer。
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// 返回该 issuer 内稳定且不透明的 subject。
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// 返回 provider 声明的邮箱；未经验证时不得用于自动创建本地账户。
    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    /// 返回 provider 是否明确验证该邮箱。
    pub const fn email_verified(&self) -> bool {
        self.email_verified
    }
}

impl std::fmt::Debug for VerifiedExternalIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedExternalIdentity")
            .field("issuer", &self.issuer)
            .field("subject", &"[REDACTED]")
            .field("email", &self.email.as_ref().map(|_| "[REDACTED]"))
            .field("email_verified", &self.email_verified)
            .finish()
    }
}

/// 一次 OIDC 授权请求及回调验证所需的短时秘密。
pub struct OidcAuthorization {
    url: url::Url,
    state: String,
    nonce: String,
    pkce_verifier: String,
}

impl OidcAuthorization {
    /// 为自定义 Provider 构造已完成安全校验的授权跳转数据。
    pub fn new(
        url: url::Url,
        state: impl Into<String>,
        nonce: impl Into<String>,
        pkce_verifier: impl Into<String>,
    ) -> Result<Self> {
        Self::new_with_loopback_http(url, state, nonce, pkce_verifier, false)
    }

    fn new_with_loopback_http(
        url: url::Url,
        state: impl Into<String>,
        nonce: impl Into<String>,
        pkce_verifier: impl Into<String>,
        allow_loopback_http: bool,
    ) -> Result<Self> {
        let state = state.into();
        let nonce = nonce.into();
        let pkce_verifier = pkce_verifier.into();
        if state.is_empty()
            || state.len() > 256
            || nonce.is_empty()
            || nonce.len() > 256
            || pkce_verifier.is_empty()
            || pkce_verifier.len() > 256
            || (url.scheme() != "https" && !(allow_loopback_http && is_loopback_http(&url)))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(oidc_error());
        }
        Ok(Self {
            url,
            state,
            nonce,
            pkce_verifier,
        })
    }

    /// 返回应将用户重定向到的 provider 授权地址。
    pub fn url(&self) -> &url::Url {
        &self.url
    }

    /// 返回需由调用方以常量时间方式与回调 state 比较的值。
    pub fn state(&self) -> &str {
        &self.state
    }

    /// 取出一次性秘密，以便服务端加密后持久化。
    pub fn into_secrets(self) -> (url::Url, String, String, String) {
        (self.url, self.state, self.nonce, self.pkce_verifier)
    }
}

impl std::fmt::Debug for OidcAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OidcAuthorization")
            .field("url", &"[REDACTED]")
            .field("state", &"[REDACTED]")
            .field("nonce", &"[REDACTED]")
            .field("pkce_verifier", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct OidcTokenFields {
    id_token: Option<String>,
}

impl std::fmt::Debug for OidcTokenFields {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OidcTokenFields")
            .field("id_token", &self.id_token.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl ExtraTokenFields for OidcTokenFields {}

type OidcTokenResponse = StandardTokenResponse<OidcTokenFields, BasicTokenType>;
const JWKS_MAX_CACHE_AGE: Duration = Duration::from_secs(300);
type ConfiguredOidcClient = Client<
    BasicErrorResponse,
    OidcTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;
type UnconfiguredOidcClient = Client<
    BasicErrorResponse,
    OidcTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
>;

#[derive(Clone, Debug, Deserialize)]
struct OidcDiscovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    id_token_signing_alg_values_supported: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct OidcClaims {
    iss: String,
    sub: String,
    aud: Audiences,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    nonce: String,
    azp: Option<String>,
    at_hash: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Audiences {
    One(String),
    Many(Vec<String>),
}

impl Audiences {
    fn contains(&self, value: &str) -> bool {
        match self {
            Self::One(audience) => audience == value,
            Self::Many(audiences) => audiences.iter().any(|audience| audience == value),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Many(audiences) => audiences.len(),
        }
    }
}

/// 使用受限出站 client 执行授权码、PKCE 与 OIDC token 验证的 Provider。
pub struct OidcProvider {
    client: RwLock<ConfiguredOidcClient>,
    jwks: RwLock<JwkSet>,
    allowed_algorithms: RwLock<HashSet<Algorithm>>,
    dependency: DependencyClient,
    total_timeout: Duration,
    allow_loopback_http: bool,
    redirect_url: RedirectUrl,
    client_id: ClientId,
    client_secret: Option<ClientSecret>,
    jwks_refresh: tokio::sync::Mutex<Option<tokio::time::Instant>>,
    jwks_last_success: tokio::sync::Mutex<tokio::time::Instant>,
    name: String,
    issuer: String,
    scopes: Vec<String>,
}

impl std::fmt::Debug for OidcProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OidcProvider")
            .field("name", &self.name)
            .field("issuer", &self.issuer)
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

impl OidcProvider {
    /// 通过受限依赖 client 执行 discovery 与 JWKS 获取并固定预期 issuer。
    #[allow(
        clippy::too_many_arguments,
        reason = "启动时需一次性提供完整且相互关联的 OIDC provider 配置"
    )]
    pub async fn discover(
        issuer: &str,
        name: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: Option<String>,
        redirect_uri: &str,
        scopes: impl IntoIterator<Item = String>,
        total_timeout: Duration,
        http: DependencyClient,
    ) -> Result<Self> {
        let name = name.into();
        if !valid_provider_name(&name) {
            return Err(oidc_error());
        }
        if !http.is_restricted() {
            return Err(oidc_error());
        }
        if total_timeout.is_zero() || total_timeout > Duration::from_secs(120) {
            return Err(oidc_error());
        }
        let callback_url = RedirectUrl::new(redirect_uri.to_owned()).map_err(|_| oidc_error())?;
        let issuer_parsed = url::Url::parse(issuer).map_err(|_| oidc_error())?;
        if issuer_parsed.query().is_some() || issuer_parsed.fragment().is_some() {
            return Err(oidc_error());
        }
        let allow_loopback_http = is_loopback_http(&issuer_parsed);
        validate_url_scheme(&issuer_parsed, allow_loopback_http)?;
        validate_url_scheme(callback_url.url(), allow_loopback_http)?;
        let client_id = ClientId::new(client_id.into());
        let client_secret = client_secret.map(ClientSecret::new);
        let deadline = Instant::now() + total_timeout;
        let (client, _discovery, jwks, allowed_algorithms) = discover_client(
            issuer.to_owned(),
            redirect_uri.to_owned(),
            client_id.clone(),
            client_secret.clone(),
            http.clone(),
            deadline,
            allow_loopback_http,
        )
        .await?;
        Ok(Self {
            client: RwLock::new(client),
            jwks: RwLock::new(jwks),
            allowed_algorithms: RwLock::new(allowed_algorithms),
            dependency: http,
            total_timeout,
            allow_loopback_http,
            redirect_url: callback_url,
            client_id,
            client_secret,
            jwks_refresh: tokio::sync::Mutex::new(None),
            jwks_last_success: tokio::sync::Mutex::new(tokio::time::Instant::now()),
            name,
            issuer: issuer.to_owned(),
            scopes: scopes.into_iter().collect(),
        })
    }

    /// 创建 authorization-code 请求，包含随机 state、nonce 和 S256 PKCE。
    pub fn authorize(&self) -> Result<OidcAuthorization> {
        let client = self.client.read().map_err(|_| oidc_error())?.clone();
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state = random_secret()?;
        let nonce = random_secret()?;
        let mut scopes = self.scopes.clone();
        if !scopes.iter().any(|scope| scope == "openid") {
            scopes.push("openid".to_owned());
        }
        let (url, _) = client
            .authorize_url(|| CsrfToken::new(state.clone()))
            .add_scopes(scopes.into_iter().map(Scope::new))
            .add_extra_param("nonce", nonce.clone())
            .set_pkce_challenge(challenge)
            .url();
        OidcAuthorization::new_with_loopback_http(
            url,
            state,
            nonce,
            verifier.secret().to_owned(),
            self.allow_loopback_http,
        )
    }

    /// 交换一次性授权码并验证签名、issuer、audience、有效期和 nonce。
    pub async fn authenticate_code(
        &self,
        callback_state: &str,
        expected_state: &str,
        code: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<VerifiedExternalIdentity> {
        use subtle::ConstantTimeEq as _;

        if callback_state.len() != expected_state.len()
            || !bool::from(callback_state.as_bytes().ct_eq(expected_state.as_bytes()))
        {
            return Err(oidc_error());
        }
        let mut client = self.client.read().map_err(|_| oidc_error())?.clone();
        let deadline = Instant::now() + self.total_timeout;
        let token = client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_owned()))
            .request_async(&RestrictedOidcHttpClient {
                dependency: self.dependency.clone(),
                deadline,
                allow_loopback_http: self.allow_loopback_http,
            })
            .await
            .map_err(|_| oidc_error())?;
        let id_token = token
            .extra_fields()
            .id_token
            .as_deref()
            .ok_or_else(oidc_error)?;
        if tokio::time::Instant::now().duration_since(*self.jwks_last_success.lock().await)
            >= JWKS_MAX_CACHE_AGE
        {
            self.refresh_jwks(deadline).await?;
            if tokio::time::Instant::now().duration_since(*self.jwks_last_success.lock().await)
                >= JWKS_MAX_CACHE_AGE
            {
                return Err(oidc_error());
            }
        }
        let mut jwks = self.jwks.read().map_err(|_| oidc_error())?.clone();
        let mut allowed_algorithms = self
            .allowed_algorithms
            .read()
            .map_err(|_| oidc_error())?
            .clone();
        let claims = match verify_oidc_token(
            id_token,
            token.access_token().secret(),
            &self.issuer,
            client.client_id().as_str(),
            nonce,
            &jwks,
            &allowed_algorithms,
        ) {
            Ok(claims) => claims,
            Err(OidcTokenError::UnknownKey) => {
                self.refresh_jwks(deadline).await?;
                let refreshed_client = self.client.read().map_err(|_| oidc_error())?;
                client.clone_from(&refreshed_client);
                jwks = self.jwks.read().map_err(|_| oidc_error())?.clone();
                let refreshed_algorithms =
                    self.allowed_algorithms.read().map_err(|_| oidc_error())?;
                allowed_algorithms.clone_from(&refreshed_algorithms);
                verify_oidc_token(
                    id_token,
                    token.access_token().secret(),
                    &self.issuer,
                    client.client_id().as_str(),
                    nonce,
                    &jwks,
                    &allowed_algorithms,
                )
                .map_err(|_| oidc_error())?
            }
            Err(_) => return Err(oidc_error()),
        };
        VerifiedExternalIdentity::new(
            claims.iss,
            claims.sub,
            claims.email,
            claims.email_verified.unwrap_or(false),
        )
    }

    async fn refresh_jwks(&self, deadline: Instant) -> Result<()> {
        let mut last_refresh = self.jwks_refresh.lock().await;
        let now = tokio::time::Instant::now();
        if last_refresh.is_some_and(|last| now.duration_since(last) < Duration::from_secs(30)) {
            return Ok(());
        }
        *last_refresh = Some(now);
        let issuer = self.issuer.clone();
        let redirect_url = self.redirect_url.as_str().to_owned();
        let client_id = self.client_id.clone();
        let client_secret = self.client_secret.clone();
        let dependency = self.dependency.clone();
        let allow_loopback_http = self.allow_loopback_http;
        let (client, discovery, jwks, allowed_algorithms) = discover_client(
            issuer,
            redirect_url,
            client_id,
            client_secret,
            dependency,
            deadline,
            allow_loopback_http,
        )
        .await?;
        if discovery.issuer != self.issuer {
            return Err(oidc_error());
        }
        *self.client.write().map_err(|_| oidc_error())? = client;
        *self.jwks.write().map_err(|_| oidc_error())? = jwks;
        *self.allowed_algorithms.write().map_err(|_| oidc_error())? = allowed_algorithms;
        *self.jwks_last_success.lock().await = tokio::time::Instant::now();
        Ok(())
    }
}

fn verify_oidc_token(
    compact_token: &str,
    access_token: &str,
    issuer: &str,
    client_id: &str,
    nonce: &str,
    jwks: &JwkSet,
    allowed_algorithms: &HashSet<Algorithm>,
) -> std::result::Result<OidcClaims, OidcTokenError> {
    use subtle::ConstantTimeEq as _;

    let header = decode_header(compact_token).map_err(|_| OidcTokenError::Invalid)?;
    if header
        .typ
        .as_deref()
        .is_some_and(|token_type| token_type != "JWT")
        || header.crit.is_some()
        || header.jwk.is_some()
        || header.jku.is_some()
        || header.x5u.is_some()
        || header.x5c.is_some()
    {
        return Err(OidcTokenError::Invalid);
    }
    let algorithm = match header.alg {
        Algorithm::RS256
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::PS256
        | Algorithm::PS384
        | Algorithm::PS512
        | Algorithm::ES256
        | Algorithm::ES384 => header.alg,
        _ => return Err(OidcTokenError::Invalid),
    };
    if !allowed_algorithms.contains(&algorithm) {
        return Err(OidcTokenError::Invalid);
    }
    let key_id = header.kid.as_deref().ok_or(OidcTokenError::Invalid)?;
    if jwks
        .keys
        .iter()
        .filter(|key| key.common.key_id.as_deref() == Some(key_id))
        .count()
        != 1
    {
        return Err(OidcTokenError::UnknownKey);
    }
    let jwk = jwks.find(key_id).ok_or(OidcTokenError::UnknownKey)?;
    validate_jwk(jwk, algorithm)?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| OidcTokenError::Invalid)?;
    let mut validation = Validation::new(algorithm);
    validation.leeway = 30;
    validation.validate_nbf = true;
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "sub", "aud"]);
    let claims = decode::<OidcClaims>(compact_token, &key, &validation)
        .map_err(|_| OidcTokenError::Invalid)?
        .claims;
    if claims.sub.trim().is_empty()
        || claims.sub.len() > 512
        || claims.iss != issuer
        || !claims.aud.contains(client_id)
        || claims.azp.as_deref().is_some_and(|azp| azp != client_id)
        || (claims.aud.len() > 1 && claims.azp.as_deref() != Some(client_id))
        || claims.exp <= jsonwebtoken::get_current_timestamp()
        || claims.iat > jsonwebtoken::get_current_timestamp().saturating_add(60)
        || claims.nbf.is_some_and(|not_before| {
            not_before > jsonwebtoken::get_current_timestamp().saturating_add(30)
        })
        || claims.nonce.len() != nonce.len()
        || !bool::from(claims.nonce.as_bytes().ct_eq(nonce.as_bytes()))
    {
        return Err(OidcTokenError::Invalid);
    }
    if let Some(expected_hash) = &claims.at_hash {
        let digest = match algorithm {
            Algorithm::RS256 | Algorithm::PS256 | Algorithm::ES256 => {
                Sha256::digest(access_token.as_bytes()).to_vec()
            }
            Algorithm::RS384 | Algorithm::PS384 | Algorithm::ES384 => {
                Sha384::digest(access_token.as_bytes()).to_vec()
            }
            Algorithm::RS512 | Algorithm::PS512 => Sha512::digest(access_token.as_bytes()).to_vec(),
            _ => return Err(OidcTokenError::Invalid),
        };
        let actual_hash = URL_SAFE_NO_PAD.encode(&digest[..digest.len() / 2]);
        if expected_hash.len() != actual_hash.len()
            || !bool::from(expected_hash.as_bytes().ct_eq(actual_hash.as_bytes()))
        {
            return Err(OidcTokenError::Invalid);
        }
    }
    Ok(claims)
}

fn validate_jwk(jwk: &Jwk, algorithm: Algorithm) -> std::result::Result<(), OidcTokenError> {
    if jwk
        .common
        .public_key_use
        .as_ref()
        .is_some_and(|usage| *usage != PublicKeyUse::Signature)
        || jwk
            .common
            .key_operations
            .as_ref()
            .is_some_and(|operations| !operations.contains(&KeyOperations::Verify))
        || jwk
            .common
            .key_algorithm
            .is_some_and(|key_algorithm| key_algorithm != KeyAlgorithm::from(algorithm))
    {
        return Err(OidcTokenError::Invalid);
    }
    Ok(())
}

#[derive(Debug)]
enum OidcTokenError {
    Invalid,
    UnknownKey,
}

async fn discover_client(
    issuer: String,
    redirect_uri: String,
    client_id: ClientId,
    client_secret: Option<ClientSecret>,
    dependency: DependencyClient,
    deadline: Instant,
    allow_loopback_http: bool,
) -> Result<(
    ConfiguredOidcClient,
    OidcDiscovery,
    JwkSet,
    HashSet<Algorithm>,
)> {
    let adapter = RestrictedOidcHttpClient {
        dependency,
        deadline,
        allow_loopback_http,
    };
    let metadata_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let metadata: OidcDiscovery = fetch_oidc_json(&adapter, &metadata_url).await?;
    if metadata.issuer != issuer {
        return Err(oidc_error());
    }
    let allowed_algorithms =
        parse_signing_algorithms(&metadata.id_token_signing_alg_values_supported)?;
    let authorization_url =
        url::Url::parse(&metadata.authorization_endpoint).map_err(|_| oidc_error())?;
    let token_url = url::Url::parse(&metadata.token_endpoint).map_err(|_| oidc_error())?;
    let jwks_url = url::Url::parse(&metadata.jwks_uri).map_err(|_| oidc_error())?;
    validate_url_scheme(&authorization_url, allow_loopback_http)?;
    validate_url_scheme(&token_url, allow_loopback_http)?;
    validate_url_scheme(&jwks_url, allow_loopback_http)?;
    let jwks: JwkSet = fetch_oidc_json(&adapter, &metadata.jwks_uri).await?;
    if jwks.keys.is_empty() || jwks.keys.len() > 64 {
        return Err(oidc_error());
    }
    let client = UnconfiguredOidcClient::new(client_id);
    let client = if let Some(secret) = client_secret {
        client.set_client_secret(secret)
    } else {
        client
    };
    let client = client
        .set_auth_uri(
            AuthUrl::new(metadata.authorization_endpoint.clone()).map_err(|_| oidc_error())?,
        )
        .set_token_uri(TokenUrl::new(metadata.token_endpoint.clone()).map_err(|_| oidc_error())?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri).map_err(|_| oidc_error())?);
    Ok((client, metadata, jwks, allowed_algorithms))
}

fn parse_signing_algorithms(values: &[String]) -> Result<HashSet<Algorithm>> {
    let supported = values
        .iter()
        .filter_map(|value| match value.as_str() {
            "RS256" => Some(Algorithm::RS256),
            "RS384" => Some(Algorithm::RS384),
            "RS512" => Some(Algorithm::RS512),
            "PS256" => Some(Algorithm::PS256),
            "PS384" => Some(Algorithm::PS384),
            "PS512" => Some(Algorithm::PS512),
            "ES256" => Some(Algorithm::ES256),
            "ES384" => Some(Algorithm::ES384),
            _ => None,
        })
        .collect::<HashSet<_>>();
    if supported.is_empty() {
        return Err(oidc_error());
    }
    Ok(supported)
}

async fn fetch_oidc_json<T: for<'de> Deserialize<'de>>(
    client: &RestrictedOidcHttpClient,
    url: &str,
) -> Result<T> {
    let request = Request::builder()
        .method(http::Method::GET)
        .uri(url)
        .body(Vec::new())
        .map_err(|_| oidc_error())?;
    let response = oauth2::AsyncHttpClient::call(client, request)
        .await
        .map_err(|_| oidc_error())?;
    if !response.status().is_success() {
        return Err(oidc_error());
    }
    serde_json::from_slice(response.body()).map_err(|_| oidc_error())
}

/// 应用可注册的外部身份 Provider contract。
pub trait ExternalIdentityProvider: Send + Sync {
    /// 返回应用配置中的稳定 Provider 名称。
    fn name(&self) -> &str;

    /// 返回 discovery 时固定的 issuer。
    fn issuer(&self) -> &str;

    /// 创建带随机 state、nonce 和 S256 PKCE 的授权请求。
    fn authorize(&self) -> Result<OidcAuthorization>;

    /// 验证回调 state 并使用一次性授权码换取已验证主体。
    fn authenticate_code<'a>(
        &'a self,
        callback_state: &'a str,
        expected_state: &'a str,
        code: &'a str,
        nonce: &'a str,
        pkce_verifier: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedExternalIdentity>>;
}

/// 已登记的外部身份登录跳转。
pub struct ExternalIdentityAuthorization {
    provider: String,
    url: url::Url,
    browser_binding: String,
    cookie_name: String,
}

impl ExternalIdentityAuthorization {
    /// 返回授权服务器跳转地址。
    pub fn url(&self) -> &url::Url {
        &self.url
    }

    /// 返回需写入短时 `HttpOnly; SameSite=Lax; Secure` Cookie 的浏览器绑定值。
    pub fn browser_binding_cookie(&self) -> &str {
        &self.browser_binding
    }

    /// 返回此授权请求专属的浏览器绑定 Cookie 名称。
    pub fn browser_binding_cookie_name(&self) -> &str {
        &self.cookie_name
    }

    /// 根据回调 state 计算发起授权时使用的浏览器绑定 Cookie 名称。
    pub fn cookie_name_for_callback(callback_state: &str) -> Result<String> {
        browser_binding_cookie_name(callback_state)
    }

    /// 返回可直接写入 HTTP 响应的十分钟浏览器绑定 `Set-Cookie` 值。
    pub fn set_cookie_header(&self) -> String {
        format!(
            "{}={}; Path=/; Max-Age=600; HttpOnly; Secure; SameSite=Lax",
            self.cookie_name, self.browser_binding
        )
    }

    /// 根据回调 state 返回已消费后用于清除 Cookie 的 `Set-Cookie` 值。
    pub fn clear_cookie_header(callback_state: &str) -> Result<String> {
        let name = browser_binding_cookie_name(callback_state)?;
        Ok(format!(
            "{name}=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax"
        ))
    }
}

impl std::fmt::Debug for ExternalIdentityAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalIdentityAuthorization")
            .field("provider", &self.provider)
            .field("url", &"[REDACTED]")
            .field("browser_binding", &"[REDACTED]")
            .field("cookie_name", &self.cookie_name)
            .finish()
    }
}

/// 当前账户已绑定的外部身份记录。
#[derive(Clone, Eq, PartialEq)]
pub struct ExternalIdentityLink {
    provider: String,
    issuer: String,
    subject: String,
}

impl ExternalIdentityLink {
    /// 返回应用配置中的 Provider 名称。
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// 返回签名验证通过的发行者标识。
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// 返回该发行者下不透明且稳定的主体标识。
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

impl std::fmt::Debug for ExternalIdentityLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalIdentityLink")
            .field("provider", &self.provider)
            .field("issuer", &self.issuer)
            .field("subject", &"[REDACTED]")
            .finish()
    }
}

/// 共享的一次性外部身份登录与账户绑定服务。
#[derive(Clone)]
pub struct ExternalIdentityFlows {
    db: Db,
    auth: AuthFlows,
    encryptor: Encryptor,
    providers: Arc<BTreeMap<String, Arc<dyn ExternalIdentityProvider>>>,
    audit: DatabaseAuditSink,
}

impl std::fmt::Debug for ExternalIdentityFlows {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalIdentityFlows")
            .field("db", &"[REDACTED]")
            .field("auth", &"[REDACTED]")
            .field("encryptor", &"[REDACTED]")
            .field("provider_count", &self.providers.len())
            .field("audit", &"[REDACTED]")
            .finish()
    }
}

impl ExternalIdentityFlows {
    /// 创建仅使用静态 provider 注册表和共享数据库的外部身份流程。
    pub fn new(
        db: Db,
        auth: AuthFlows,
        encryptor: Encryptor,
        providers: impl IntoIterator<Item = Arc<dyn ExternalIdentityProvider>>,
    ) -> Result<Self> {
        let mut registry = BTreeMap::new();
        for provider in providers {
            let name = provider.name().to_owned();
            if !valid_provider_name(&name) || registry.insert(name, provider).is_some() {
                return Err(oidc_error());
            }
        }
        if registry.is_empty() {
            return Err(oidc_error());
        }
        Ok(Self {
            audit: DatabaseAuditSink::new(db.clone()),
            db,
            auth,
            encryptor,
            providers: Arc::new(registry),
        })
    }

    /// 创建十分钟有效的一次性登录事务并返回授权跳转地址。
    pub async fn authorize(
        &self,
        provider_name: &str,
        request_source: &str,
    ) -> Result<ExternalIdentityAuthorization> {
        self.auth
            .limit_external_identity(request_source, provider_name)
            .await?;
        let provider = self.provider(provider_name)?;
        let authorization = provider.authorize()?;
        let (url, state, nonce, pkce_verifier) = authorization.into_secrets();
        let state_hash = digest(&state);
        let cookie_name = browser_binding_cookie_name(&state)?;
        let browser_binding = random_secret()?;
        let browser_binding_hash = digest(&browser_binding);
        let context = request_context(&state_hash);
        let payload = serde_json::to_vec(&PendingOidcRequest {
            state,
            nonce,
            pkce_verifier,
        })
        .map_err(Error::internal)?;
        let encrypted_payload = self
            .encryptor
            .encrypt_with_context(&payload, context.as_bytes())?;
        let table = self.db.quoted_table_name("external_identity_requests")?;
        let backend = self.db.backend()?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "INSERT INTO {table} (state_hash, provider, browser_binding_hash, encrypted_payload, expires_at, created_at) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP + INTERVAL '10 minutes', CURRENT_TIMESTAMP)"
            ),
            DbBackend::MySql => format!(
                "INSERT INTO {table} (state_hash, provider, browser_binding_hash, encrypted_payload, expires_at, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP(6) + INTERVAL 10 MINUTE, CURRENT_TIMESTAMP(6))"
            ),
            _ => return Err(oidc_error()),
        };
        self.db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [
                    state_hash.into(),
                    provider_name.to_owned().into(),
                    browser_binding_hash.into(),
                    encrypted_payload.into(),
                ],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(ExternalIdentityAuthorization {
            provider: provider_name.to_owned(),
            url,
            browser_binding,
            cookie_name,
        })
    }

    /// 列出本地账户已绑定的外部身份，供账户解绑界面展示。
    pub async fn linked_identities(&self, user_id: i64) -> Result<Vec<ExternalIdentityLink>> {
        if user_id <= 0 {
            return Err(oidc_error());
        }
        let table = self.db.quoted_table_name("external_identities")?;
        let backend = self.db.backend()?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "SELECT provider, issuer, subject FROM {table} WHERE user_id = $1 ORDER BY id"
            ),
            DbBackend::MySql => format!(
                "SELECT provider, issuer, subject FROM {table} WHERE user_id = ? ORDER BY id"
            ),
            _ => return Err(oidc_error()),
        };
        self.db
            .connection()?
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .into_iter()
            .map(|row| {
                Ok(ExternalIdentityLink {
                    provider: row.try_get("", "provider").map_err(Error::internal)?,
                    issuer: row.try_get("", "issuer").map_err(Error::internal)?,
                    subject: row.try_get("", "subject").map_err(Error::internal)?,
                })
            })
            .collect()
    }

    /// 验证 provider 回调并原子消费 state；启用 MFA 时只返回一次性 challenge。
    #[allow(
        clippy::too_many_lines,
        reason = "state 消费、OIDC principal 验证与凭据签发必须保持同一事务流程"
    )]
    pub async fn complete(
        &self,
        provider_name: &str,
        callback_state: &str,
        code: &str,
        browser_binding: &str,
        request_source: &str,
    ) -> Result<LoginAttempt> {
        if callback_state.is_empty()
            || callback_state.len() > 256
            || code.is_empty()
            || code.len() > 4096
            || browser_binding.is_empty()
            || browser_binding.len() > 256
        {
            return Err(oidc_error());
        }
        let state_hash = digest(callback_state);
        self.auth
            .limit_external_identity(request_source, &state_hash)
            .await?;
        let table = self.db.quoted_table_name("external_identity_requests")?;
        let backend = self.db.backend()?;
        let (lookup, values) = match backend {
            DbBackend::Postgres => (
                format!(
                    "SELECT provider, browser_binding_hash, encrypted_payload FROM {table} WHERE state_hash = $1 AND expires_at > CURRENT_TIMESTAMP"
                ),
                vec![state_hash.clone().into()],
            ),
            DbBackend::MySql => (
                format!(
                    "SELECT provider, browser_binding_hash, encrypted_payload FROM {table} WHERE state_hash = ? AND expires_at > CURRENT_TIMESTAMP(6)"
                ),
                vec![state_hash.clone().into()],
            ),
            _ => return Err(oidc_error()),
        };
        let row = self
            .db
            .connection()?
            .query_one_raw(Statement::from_sql_and_values(backend, &lookup, values))
            .await
            .map_err(Error::internal)?
            .ok_or_else(oidc_error)?;
        let stored_provider = row
            .try_get::<String>("", "provider")
            .map_err(Error::internal)?;
        if stored_provider != provider_name {
            return Err(oidc_error());
        }
        verify_browser_binding(
            &row.try_get::<String>("", "browser_binding_hash")
                .map_err(Error::internal)?,
            browser_binding,
        )?;
        let encrypted = row
            .try_get::<String>("", "encrypted_payload")
            .map_err(Error::internal)?;
        let context = request_context(&state_hash);
        let payload = self
            .encryptor
            .decrypt_with_context(&encrypted, context.as_bytes())?;
        let pending: PendingOidcRequest =
            serde_json::from_slice(&payload).map_err(|_| oidc_error())?;
        verify_state(callback_state, &pending.state)?;
        let provider = self.provider(provider_name)?;
        let identity = provider
            .authenticate_code(
                callback_state,
                &pending.state,
                code,
                &pending.nonce,
                &pending.pkce_verifier,
            )
            .await?;
        if identity.issuer() != provider.issuer() {
            return Err(oidc_error());
        }
        let password_hash = if self
            .find_external_user(provider_name, &identity)
            .await?
            .is_none()
        {
            Some(self.auth.prepare_external_password_hash().await?)
        } else {
            None
        };
        let auth = self.auth.clone();
        let audit = self.audit.clone();
        let db = self.db.clone();
        let provider_name = provider_name.to_owned();
        let request_source = request_source.to_owned();
        self.db
            .transaction(|transaction| {
                let auth = auth.clone();
                let audit = audit.clone();
                let db = db.clone();
                let provider_name = provider_name.clone();
                let identity = identity.clone();
                let password_hash = password_hash.clone();
                let state_hash = state_hash.clone();
                let request_source = request_source.clone();
                Box::pin(async move {
                    consume_request(transaction, &db, &provider_name, &state_hash).await?;
                    let credentials = auth
                        .complete_external_identity_in_transaction(
                            transaction,
                            &provider_name,
                            &identity,
                            password_hash.as_ref(),
                            &request_source,
                        )
                        .await?;
                    let event = AuditEvent::new(
                        if matches!(&credentials, LoginAttempt::Authenticated(_)) {
                            "auth.external.login"
                        } else {
                            "auth.external.challenge_issued"
                        },
                        AuditSubject::new("external_provider", &provider_name)?,
                        AuditOutcome::Succeeded,
                    )?
                    .actor(AuditSubject::new(
                        "user",
                        credentials.user_id().to_string(),
                    )?);
                    let record =
                        Audit::new(audit.clone(), AuditPolicy::default()).prepare(event)?;
                    audit.record_in_transaction(transaction, &record).await?;
                    Ok(credentials)
                })
            })
            .await
    }

    /// 经当前密码重新认证和一次性 OIDC 回调后，将身份绑定到本地账户。
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "绑定同时重新认证本地用户、消费浏览器回调并事务提交身份与审计"
    )]
    pub async fn bind(
        &self,
        user_id: i64,
        email: &str,
        password: &str,
        request_source: &str,
        provider_name: &str,
        callback_state: &str,
        code: &str,
        browser_binding: &str,
    ) -> Result<()> {
        let provider = self.provider(provider_name)?;
        if user_id <= 0
            || callback_state.is_empty()
            || callback_state.len() > 256
            || code.is_empty()
            || code.len() > 4096
            || browser_binding.is_empty()
            || browser_binding.len() > 256
        {
            return Err(oidc_error());
        }
        if self
            .auth
            .reauthenticate(email, password, request_source)
            .await?
            != user_id
        {
            return Err(Error::Unauthorized);
        }
        let state_hash = digest(callback_state);
        self.auth
            .limit_external_identity(request_source, &state_hash)
            .await?;
        let table = self.db.quoted_table_name("external_identity_requests")?;
        let backend = self.db.backend()?;
        let lookup = match backend {
            DbBackend::Postgres => format!(
                "SELECT provider, browser_binding_hash, encrypted_payload FROM {table} WHERE state_hash = $1 AND expires_at > CURRENT_TIMESTAMP"
            ),
            DbBackend::MySql => format!(
                "SELECT provider, browser_binding_hash, encrypted_payload FROM {table} WHERE state_hash = ? AND expires_at > CURRENT_TIMESTAMP(6)"
            ),
            _ => return Err(oidc_error()),
        };
        let row = self
            .db
            .connection()?
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &lookup,
                [state_hash.clone().into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(oidc_error)?;
        let stored_provider = row
            .try_get::<String>("", "provider")
            .map_err(Error::internal)?;
        if stored_provider != provider_name {
            return Err(oidc_error());
        }
        let binding_hash = row
            .try_get::<String>("", "browser_binding_hash")
            .map_err(Error::internal)?;
        verify_browser_binding(&binding_hash, browser_binding)?;
        let encrypted = row
            .try_get::<String>("", "encrypted_payload")
            .map_err(Error::internal)?;
        let payload = self
            .encryptor
            .decrypt_with_context(&encrypted, request_context(&state_hash).as_bytes())?;
        let pending: PendingOidcRequest =
            serde_json::from_slice(&payload).map_err(|_| oidc_error())?;
        verify_state(callback_state, &pending.state)?;
        let identity = provider
            .authenticate_code(
                callback_state,
                &pending.state,
                code,
                &pending.nonce,
                &pending.pkce_verifier,
            )
            .await?;
        if provider.issuer() != identity.issuer() {
            return Err(oidc_error());
        }
        let db = self.db.clone();
        let audit = self.audit.clone();
        let provider_name = provider_name.to_owned();
        let identity_hash = identity.identity_hash(&provider_name);
        let issuer = identity.issuer().to_owned();
        let subject = identity.subject().to_owned();
        self.db
            .transaction(|transaction| {
                let db = db.clone();
                let audit = audit.clone();
                let provider_name = provider_name.clone();
                let identity_hash = identity_hash.clone();
                let state_hash = state_hash.clone();
                Box::pin(async move {
                    consume_request(transaction, &db, &provider_name, &state_hash).await?;
                    let table = db.quoted_table_name("external_identities")?;
                    let backend = db.backend()?;
                    let sql = match backend {
                        DbBackend::Postgres => format!(
                            "INSERT INTO {table} (user_id, identity_hash, provider, issuer, subject, created_at) VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP) ON CONFLICT (identity_hash) DO NOTHING"
                        ),
                        DbBackend::MySql => format!(
                            "INSERT INTO {table} (user_id, identity_hash, provider, issuer, subject, created_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP(6))"
                        ),
                        _ => return Err(oidc_error()),
                    };
                    let result = transaction
                        .execute_raw(Statement::from_sql_and_values(
                            backend,
                            &sql,
                            [
                                user_id.into(),
                                identity_hash.into(),
                                provider_name.clone().into(),
                                issuer.into(),
                                subject.into(),
                            ],
                        ))
                        .await;
                    let inserted = match result {
                        Ok(result) => result.rows_affected() == 1,
                        Err(error)
                            if matches!(
                                error.sql_err(),
                                Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
                            ) => false,
                        Err(error) => return Err(Error::internal(error)),
                    };
                    if !inserted {
                        return Err(identity_link_conflict());
                    }
                    record_identity_audit(
                        &audit,
                        transaction,
                        user_id,
                        "auth.external.bind",
                        &provider_name,
                    )
                    .await
                })
            })
            .await
    }

    /// 经当前密码重新认证后，解除本地账户的指定外部身份绑定。
    #[allow(
        clippy::too_many_arguments,
        reason = "解除绑定需要明确提供账户、provider、issuer 与 subject"
    )]
    pub async fn unbind(
        &self,
        user_id: i64,
        email: &str,
        password: &str,
        request_source: &str,
        provider_name: &str,
        issuer: &str,
        subject: &str,
    ) -> Result<bool> {
        self.provider(provider_name)?;
        if user_id <= 0
            || issuer.is_empty()
            || subject.is_empty()
            || self.provider(provider_name)?.issuer() != issuer
        {
            return Err(oidc_error());
        }
        if self
            .auth
            .reauthenticate(email, password, request_source)
            .await?
            != user_id
        {
            return Err(Error::Unauthorized);
        }
        let db = self.db.clone();
        let audit = self.audit.clone();
        let provider_name = provider_name.to_owned();
        let identity_hash = external_identity_hash(&provider_name, issuer, subject);
        self.db
            .transaction(|transaction| {
                let db = db.clone();
                let audit = audit.clone();
                let provider_name = provider_name.clone();
                let identity_hash = identity_hash.clone();
                Box::pin(async move {
                    let table = db.quoted_table_name("external_identities")?;
                    let backend = db.backend()?;
                    let sql = match backend {
                        DbBackend::Postgres => {
                            format!("DELETE FROM {table} WHERE user_id = $1 AND identity_hash = $2")
                        }
                        DbBackend::MySql => {
                            format!("DELETE FROM {table} WHERE user_id = ? AND identity_hash = ?")
                        }
                        _ => return Err(oidc_error()),
                    };
                    let result = transaction
                        .execute_raw(Statement::from_sql_and_values(
                            backend,
                            &sql,
                            [user_id.into(), identity_hash.into()],
                        ))
                        .await
                        .map_err(Error::internal)?;
                    if result.rows_affected() == 0 {
                        return Ok(false);
                    }
                    record_identity_audit(
                        &audit,
                        transaction,
                        user_id,
                        "auth.external.unbind",
                        &provider_name,
                    )
                    .await?;
                    Ok(true)
                })
            })
            .await
    }

    /// 删除最多指定数量的过期登录事务。
    pub async fn purge_expired(&self, batch_size: u16) -> Result<u64> {
        if batch_size == 0 || batch_size > 1000 {
            return Err(Error::BadRequest(
                "invalid external identity cleanup batch size".into(),
            ));
        }
        let table = self.db.quoted_table_name("external_identity_requests")?;
        let backend = self.db.backend()?;
        let (sql, values) = match backend {
            DbBackend::Postgres => (
                format!(
                    "DELETE FROM {table} WHERE state_hash IN (SELECT state_hash FROM {table} WHERE expires_at <= CURRENT_TIMESTAMP ORDER BY expires_at LIMIT $1)"
                ),
                vec![i64::from(batch_size).into()],
            ),
            DbBackend::MySql => (
                format!(
                    "DELETE FROM {table} WHERE state_hash IN (SELECT state_hash FROM (SELECT state_hash FROM {table} WHERE expires_at <= CURRENT_TIMESTAMP(6) ORDER BY expires_at LIMIT ?) expired_requests)"
                ),
                vec![i64::from(batch_size).into()],
            ),
            _ => return Err(oidc_error()),
        };
        Ok(self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
            .await
            .map_err(Error::internal)?
            .rows_affected())
    }

    fn provider(&self, name: &str) -> Result<&Arc<dyn ExternalIdentityProvider>> {
        self.providers.get(name).ok_or_else(oidc_error)
    }

    async fn find_external_user(
        &self,
        provider: &str,
        identity: &VerifiedExternalIdentity,
    ) -> Result<Option<i64>> {
        let table = self.db.quoted_table_name("external_identities")?;
        let backend = self.db.backend()?;
        let sql = match backend {
            DbBackend::Postgres => format!("SELECT user_id FROM {table} WHERE identity_hash = $1"),
            DbBackend::MySql => format!("SELECT user_id FROM {table} WHERE identity_hash = ?"),
            _ => return Err(oidc_error()),
        };
        self.db
            .connection()?
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [identity.identity_hash(provider).into()],
            ))
            .await
            .map_err(Error::internal)?
            .map(|row| row.try_get::<i64>("", "user_id").map_err(Error::internal))
            .transpose()
    }
}

async fn record_identity_audit(
    audit: &DatabaseAuditSink,
    transaction: &DatabaseTransaction,
    user_id: i64,
    action: &str,
    provider: &str,
) -> Result<()> {
    let event = AuditEvent::new(
        action,
        AuditSubject::new("external_provider", provider)?,
        AuditOutcome::Succeeded,
    )?
    .actor(AuditSubject::new("user", user_id.to_string())?);
    let record = Audit::new(audit.clone(), AuditPolicy::default()).prepare(event)?;
    audit.record_in_transaction(transaction, &record).await
}

#[derive(Serialize, Deserialize)]
struct PendingOidcRequest {
    state: String,
    nonce: String,
    pkce_verifier: String,
}

async fn consume_request(
    transaction: &DatabaseTransaction,
    db: &Db,
    provider: &str,
    state_hash: &str,
) -> Result<()> {
    let table = db.quoted_table_name("external_identity_requests")?;
    let backend = db.backend()?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "DELETE FROM {table} WHERE state_hash = $1 AND provider = $2 AND expires_at > CURRENT_TIMESTAMP"
        ),
        DbBackend::MySql => format!(
            "DELETE FROM {table} WHERE state_hash = ? AND provider = ? AND expires_at > CURRENT_TIMESTAMP(6)"
        ),
        _ => return Err(oidc_error()),
    };
    let result = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [state_hash.to_owned().into(), provider.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?;
    if result.rows_affected() != 1 {
        return Err(oidc_error());
    }
    Ok(())
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn external_identity_hash(provider: &str, issuer: &str, subject: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(provider.as_bytes());
    digest.update([0]);
    digest.update(issuer.as_bytes());
    digest.update([0]);
    digest.update(subject.as_bytes());
    format!("{:x}", digest.finalize())
}

fn request_context(state_hash: &str) -> String {
    format!("auth.external_identity_requests:{state_hash}")
}

fn identity_link_conflict() -> Error {
    Error::custom(
        "auth_external_identity_conflict",
        "external identity is already linked to another account",
    )
}

impl ExternalIdentityProvider for OidcProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn issuer(&self) -> &str {
        &self.issuer
    }

    fn authorize(&self) -> Result<OidcAuthorization> {
        Self::authorize(self)
    }

    fn authenticate_code<'a>(
        &'a self,
        callback_state: &'a str,
        expected_state: &'a str,
        code: &'a str,
        nonce: &'a str,
        pkce_verifier: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedExternalIdentity>> {
        Box::pin(async move {
            Self::authenticate_code(
                self,
                callback_state,
                expected_state,
                code,
                nonce,
                pkce_verifier,
            )
            .await
        })
    }
}

struct RestrictedOidcHttpClient {
    dependency: DependencyClient,
    deadline: Instant,
    allow_loopback_http: bool,
}

impl<'c> oauth2::AsyncHttpClient<'c> for RestrictedOidcHttpClient {
    type Error = OidcHttpError;
    type Future = Pin<
        Box<dyn Future<Output = std::result::Result<Response<Vec<u8>>, Self::Error>> + Send + 'c>,
    >;

    fn call(&'c self, request: Request<Vec<u8>>) -> Self::Future {
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            if body.len() > MAX_OIDC_BODY_BYTES {
                return Err(OidcHttpError::Response);
            }
            let uri = parts.uri.to_string();
            let parsed = url::Url::parse(&uri).map_err(|_| OidcHttpError::Target)?;
            if validate_url_scheme(&parsed, self.allow_loopback_http).is_err() {
                return Err(OidcHttpError::Target);
            }
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .filter(|duration| !duration.is_zero())
                .ok_or(OidcHttpError::Timeout)?;
            let request = self
                .dependency
                .request(parts.method, &uri)
                .timeout(remaining)
                .headers(parts.headers)
                .body(HttpBody::from(body));
            let mut response = request.send().await.map_err(|_| OidcHttpError::Client)?;
            let status = response.status();
            let headers = response.headers().clone();
            let mut body = Vec::new();
            while let Some(chunk) = response
                .next_chunk()
                .await
                .map_err(|_| OidcHttpError::Client)?
            {
                if body.len().saturating_add(chunk.len()) > MAX_OIDC_BODY_BYTES {
                    return Err(OidcHttpError::Response);
                }
                body.extend_from_slice(&chunk);
            }
            let mut response = Response::builder()
                .status(status)
                .body(body)
                .map_err(|_| OidcHttpError::Response)?;
            *response.headers_mut() = headers;
            Ok(response)
        })
    }
}

#[derive(Debug)]
enum OidcHttpError {
    Client,
    Response,
    Target,
    Timeout,
}

const MAX_OIDC_BODY_BYTES: usize = 1024 * 1024;

fn random_secret() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn browser_binding_cookie_name(callback_state: &str) -> Result<String> {
    if callback_state.is_empty() || callback_state.len() > 256 {
        return Err(oidc_error());
    }
    let hash = digest(callback_state);
    Ok(format!("auth_oidc_{}", &hash[..24]))
}

fn verify_browser_binding(expected_hash: &str, supplied: &str) -> Result<()> {
    use subtle::ConstantTimeEq as _;
    let actual_hash = digest(supplied);
    if expected_hash.len() != actual_hash.len()
        || !bool::from(expected_hash.as_bytes().ct_eq(actual_hash.as_bytes()))
    {
        return Err(oidc_error());
    }
    Ok(())
}

fn verify_state(callback_state: &str, expected_state: &str) -> Result<()> {
    use subtle::ConstantTimeEq as _;
    if callback_state.len() != expected_state.len()
        || !bool::from(callback_state.as_bytes().ct_eq(expected_state.as_bytes()))
    {
        return Err(oidc_error());
    }
    Ok(())
}

fn validate_url_scheme(url: &url::Url, allow_loopback_http: bool) -> Result<()> {
    let valid_scheme = url.scheme() == "https"
        || (allow_loopback_http && url.scheme() == "http" && is_loopback_http(url));
    if !valid_scheme || url.username() != "" || url.password().is_some() {
        return Err(oidc_error());
    }
    Ok(())
}

fn is_loopback_http(url: &url::Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback()),
        None => false,
    }
}

fn valid_provider_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

impl std::fmt::Display for OidcHttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OIDC restricted HTTP request failed")
    }
}

impl std::error::Error for OidcHttpError {}

fn oidc_error() -> Error {
    Error::custom(
        "auth_oidc_failed",
        "external identity authentication failed",
    )
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{EncodingKey, Header, encode, jwk::Jwk};
    use serde_json::json;

    use super::*;

    const PRIVATE_KEY: &[u8] = include_bytes!("testdata/oidc-es256-private.pem");

    fn token(
        key: &EncodingKey,
        key_id: &str,
        issuer: &str,
        audience: &str,
        nonce: &str,
        at_hash: Option<&str>,
    ) -> std::result::Result<String, jsonwebtoken::errors::Error> {
        let now = jsonwebtoken::get_current_timestamp();
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(key_id.to_owned());
        encode(
            &header,
            &json!({
                "iss": issuer,
                "sub": "subject-1",
                "aud": audience,
                "exp": now + 600,
                "iat": now,
                "nonce": nonce,
                "at_hash": at_hash,
                "email": "verified@example.test",
                "email_verified": true
            }),
            key,
        )
    }

    fn test_keyset(key: &EncodingKey, key_id: &str) -> Result<JwkSet> {
        let mut jwk = Jwk::from_encoding_key(key, Algorithm::ES256).map_err(Error::internal)?;
        jwk.common.key_id = Some(key_id.to_owned());
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);
        Ok(JwkSet { keys: vec![jwk] })
    }

    #[test]
    fn id_token_requires_signature_issuer_audience_nonce_and_access_token_hash() -> Result<()> {
        let key = EncodingKey::from_ec_pem(PRIVATE_KEY).map_err(Error::internal)?;
        let jwks = test_keyset(&key, "signing-1")?;
        let access_token = "opaque-access-token";
        let digest = Sha256::digest(access_token.as_bytes());
        let at_hash = URL_SAFE_NO_PAD.encode(&digest[..digest.len() / 2]);
        let signed = token(
            &key,
            "signing-1",
            "https://issuer.example.test",
            "openrust-client",
            "expected-nonce",
            Some(&at_hash),
        )
        .map_err(Error::internal)?;

        let claims = verify_oidc_token(
            &signed,
            access_token,
            "https://issuer.example.test",
            "openrust-client",
            "expected-nonce",
            &jwks,
            &HashSet::from([Algorithm::ES256]),
        )
        .map_err(|_| oidc_error())?;
        assert_eq!(claims.sub, "subject-1");
        assert_eq!(claims.email.as_deref(), Some("verified@example.test"));
        assert!(claims.email_verified.unwrap_or(false));

        assert!(
            verify_oidc_token(
                &signed,
                access_token,
                "https://issuer.example.test",
                "openrust-client",
                "wrong-nonce",
                &jwks,
                &HashSet::from([Algorithm::ES256]),
            )
            .is_err()
        );
        assert!(
            verify_oidc_token(
                &signed,
                access_token,
                "https://wrong-issuer.example.test",
                "openrust-client",
                "expected-nonce",
                &jwks,
                &HashSet::from([Algorithm::ES256]),
            )
            .is_err()
        );
        assert!(
            verify_oidc_token(
                &signed,
                "wrong-access-token",
                "https://issuer.example.test",
                "openrust-client",
                "expected-nonce",
                &jwks,
                &HashSet::from([Algorithm::ES256]),
            )
            .is_err()
        );
        assert!(matches!(
            verify_oidc_token(
                &signed,
                access_token,
                "https://issuer.example.test",
                "openrust-client",
                "expected-nonce",
                &JwkSet { keys: Vec::new() },
                &HashSet::from([Algorithm::ES256]),
            ),
            Err(OidcTokenError::UnknownKey)
        ));
        Ok(())
    }

    #[test]
    fn authorization_constructor_rejects_malformed_or_unsafe_urls() -> Result<()> {
        let https =
            url::Url::parse("https://idp.example.test/authorize").map_err(Error::internal)?;
        assert!(OidcAuthorization::new(https, "state", "nonce", "verifier").is_ok());
        let file = url::Url::parse("file:///etc/passwd").map_err(Error::internal)?;
        assert!(OidcAuthorization::new(file, "state", "nonce", "verifier").is_err());
        let https =
            url::Url::parse("https://idp.example.test/authorize").map_err(Error::internal)?;
        assert!(OidcAuthorization::new(https, "", "nonce", "verifier").is_err());
        Ok(())
    }

    #[test]
    fn token_debug_output_redacts_id_token() {
        let token = OidcTokenFields {
            id_token: Some("header.payload.signature".to_owned()),
        };
        let formatted = format!("{token:?}");
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains("header.payload.signature"));
    }
}
