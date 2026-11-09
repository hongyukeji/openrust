//! 基于授权码与 PKCE 的 `OpenID Connect` relying-party 客户端。

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::{Arc, RwLock},
};
use std::{net::IpAddr, time::Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration as ChronoDuration, Utc};
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
use crate::auth::{AuthFlows, LoginAttempt, Session, SessionStoreHandle};
use crate::core::{Error, Result};
use crate::database::{DatabaseTransaction, Db};
use crate::encryption::ContextualEncrypter;
use crate::http::{DependencyClient, HttpBody};
use crate::support::BoxFuture;

/// 通过 ID Token 验证的外部身份；仅包含稳定 subject 与可选邮箱声明。
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedExternalIdentity {
    issuer: String,
    subject: String,
    email: Option<String>,
    email_verified: bool,
    session_id: Option<String>,
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
            session_id: None,
        })
    }

    /// 设置已由 Provider 验证的 `IdP` 会话标识，用于精确处理后通道退出。
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Result<Self> {
        let session_id = session_id.into();
        if session_id.trim().is_empty()
            || session_id.len() > 512
            || session_id.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(oidc_error());
        }
        self.session_id = Some(session_id);
        Ok(self)
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

    /// 返回经签名验证的 `IdP` 会话标识（若 Provider 提供）。
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
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
            .field(
                "session_id",
                &self.session_id.as_ref().map(|_| "[REDACTED]"),
            )
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
    end_session_endpoint: Option<String>,
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
    sid: Option<String>,
}

#[derive(Deserialize)]
struct OidcLogoutClaims {
    iss: String,
    aud: Audiences,
    exp: u64,
    iat: u64,
    jti: String,
    events: BTreeMap<String, serde_json::Value>,
    nonce: Option<String>,
    sid: Option<String>,
    sub: Option<String>,
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
    end_session_endpoint: Option<url::Url>,
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
        let (client, discovery, jwks, allowed_algorithms) = discover_client(
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
            end_session_endpoint: discovery
                .end_session_endpoint
                .as_deref()
                .map(url::Url::parse)
                .transpose()
                .map_err(|_| oidc_error())?,
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
        let mut identity = VerifiedExternalIdentity::new(
            claims.iss,
            claims.sub,
            claims.email,
            claims.email_verified.unwrap_or(false),
        )?;
        if let Some(session_id) = claims.sid {
            identity = identity.with_session_id(session_id)?;
        }
        Ok(identity)
    }

    fn verify_backchannel_logout_token(&self, compact: &str) -> Result<VerifiedLogoutToken> {
        let jwks = self.jwks.read().map_err(|_| oidc_error())?.clone();
        let algorithms = self
            .allowed_algorithms
            .read()
            .map_err(|_| oidc_error())?
            .clone();
        Self::verify_logout_token(
            compact,
            &self.issuer,
            self.client_id.as_str(),
            &jwks,
            &algorithms,
        )
    }

    fn verify_logout_token(
        compact: &str,
        issuer: &str,
        client_id: &str,
        jwks: &JwkSet,
        allowed_algorithms: &HashSet<Algorithm>,
    ) -> Result<VerifiedLogoutToken> {
        let header = decode_header(compact).map_err(|_| oidc_error())?;
        if header.typ.as_deref() != Some("logout+jwt")
            || header.crit.is_some()
            || header.jwk.is_some()
            || header.jku.is_some()
            || header.x5u.is_some()
            || header.x5c.is_some()
            || !allowed_algorithms.contains(&header.alg)
        {
            return Err(oidc_error());
        }
        let kid = header.kid.as_deref().ok_or_else(oidc_error)?;
        if jwks
            .keys
            .iter()
            .filter(|key| key.common.key_id.as_deref() == Some(kid))
            .count()
            != 1
        {
            return Err(oidc_error());
        }
        let jwk = jwks.find(kid).ok_or_else(oidc_error)?;
        validate_jwk(jwk, header.alg).map_err(|_| oidc_error())?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| oidc_error())?;
        let mut validation = Validation::new(header.alg);
        validation.leeway = 30;
        validation.validate_nbf = true;
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[client_id]);
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        let claims = decode::<OidcLogoutClaims>(compact, &key, &validation)
            .map_err(|_| oidc_error())?
            .claims;
        let now = jsonwebtoken::get_current_timestamp();
        let event_name = "http://schemas.openid.net/event/backchannel-logout";
        if claims.iss != issuer
            || !claims.aud.contains(client_id)
            || claims.jti.trim().is_empty()
            || claims.jti.len() > 256
            || claims.jti.bytes().any(|byte| byte.is_ascii_control())
            || claims.iat > now.saturating_add(60)
            || now.saturating_sub(claims.iat) > 300
            || claims.exp <= now
            || claims.nonce.is_some()
            || claims.sid.as_ref().is_some_and(|sid| {
                sid.is_empty() || sid.len() > 512 || sid.bytes().any(|byte| byte.is_ascii_control())
            })
            || claims.sub.as_ref().is_some_and(|sub| {
                sub.is_empty() || sub.len() > 512 || sub.bytes().any(|byte| byte.is_ascii_control())
            })
            || (claims.sid.is_none() && claims.sub.is_none())
            || !claims
                .events
                .get(event_name)
                .is_some_and(|event| event.as_object().is_some_and(serde_json::Map::is_empty))
        {
            return Err(oidc_error());
        }
        Ok(VerifiedLogoutToken {
            issuer: claims.iss,
            client_id: client_id.to_owned(),
            token_id: claims.jti,
            issued_at: claims.iat,
            session_id: claims.sid,
            subject: claims.sub,
        })
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
    if let Some(endpoint) = &metadata.end_session_endpoint {
        let endpoint = url::Url::parse(endpoint).map_err(|_| oidc_error())?;
        validate_url_scheme(&endpoint, allow_loopback_http)?;
        if !endpoint.username().is_empty() || endpoint.password().is_some() {
            return Err(oidc_error());
        }
    }
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

    /// 返回经 discovery 固定并验证的 RP-Initiated Logout endpoint。
    fn end_session_endpoint(&self) -> Option<url::Url> {
        None
    }

    /// 返回与该客户端预登记的退出后回调 URI；自定义 Provider 默认不允许重定向。
    fn post_logout_redirect_uri(&self) -> Option<String> {
        None
    }

    /// 返回固定 client ID，用于 RP-Initiated Logout 请求。
    fn client_id(&self) -> Option<&str> {
        None
    }

    /// 独立验证 Back-Channel Logout Token；自定义 Provider 默认拒绝该能力。
    fn verify_logout_token<'a>(
        &'a self,
        _token: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedLogoutToken>> {
        Box::pin(async { Err(oidc_error()) })
    }
}

/// 已按 OIDC Back-Channel Logout 规则验证的通知声明。
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedLogoutToken {
    issuer: String,
    client_id: String,
    token_id: String,
    issued_at: u64,
    session_id: Option<String>,
    subject: Option<String>,
}

impl VerifiedLogoutToken {
    /// 由自定义 Provider 在完成签名、issuer、audience、时效、事件及抗重放声明校验后构造。
    pub fn after_provider_validation(
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        token_id: impl Into<String>,
        issued_at: u64,
        session_id: Option<String>,
        subject: Option<String>,
    ) -> Result<Self> {
        let issuer = issuer.into();
        let client_id = client_id.into();
        let token_id = token_id.into();
        if issuer.trim().is_empty()
            || issuer.len() > 512
            || client_id.trim().is_empty()
            || client_id.len() > 512
            || token_id.trim().is_empty()
            || token_id.len() > 256
            || (session_id.is_none() && subject.is_none())
            || session_id
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 512)
            || subject
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 512)
        {
            return Err(oidc_error());
        }
        Ok(Self {
            issuer,
            client_id,
            token_id,
            issued_at,
            session_id,
            subject,
        })
    }

    /// 返回已验证的 issuer。
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    /// 返回该通知的目标 client。
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    /// 返回用于跨节点重放去重的 token ID。
    pub fn token_id(&self) -> &str {
        &self.token_id
    }
    /// 返回签发时间。
    pub const fn issued_at(&self) -> u64 {
        self.issued_at
    }
    /// 返回精确 `IdP` 会话 ID。
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
    /// 返回 subject 范围通知的主体。
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
}

impl std::fmt::Debug for VerifiedLogoutToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedLogoutToken")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("token_id", &"[REDACTED]")
            .field("issued_at", &self.issued_at)
            .field(
                "session_id",
                &self.session_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("subject", &self.subject.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// 已登记的外部身份登录跳转。
pub struct ExternalIdentityAuthorization {
    provider: String,
    url: url::Url,
    browser_binding: String,
    cookie_name: String,
}

/// 一次外部登录与本地应用凭据关联的短期不透明句柄。
pub struct ExternalLoginHandle(String);

impl ExternalLoginHandle {
    /// 暴露一次性应用关联凭据，供当前登录流程的后续步骤提交。
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// 从应用安全存储中的一次性凭据恢复关联句柄，供多实例登录流程继续使用。
    ///
    /// 只接受框架签发的 256-bit Base64 URL-safe 无填充格式；调用方须将该值作为秘密保存。
    pub fn from_exposed(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() != 43
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(oidc_error());
        }
        Ok(Self(value))
    }
}

impl std::fmt::Debug for ExternalLoginHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExternalLoginHandle([REDACTED])")
    }
}

/// 外部身份登录结果及其供 Session/refresh 关联使用的句柄。
pub struct ExternalLoginResult {
    attempt: LoginAttempt,
    handle: ExternalLoginHandle,
}

/// 已完成本地撤销并可跳转到 `IdP` 的退出地址。
pub struct ExternalLogoutRedirect {
    url: url::Url,
    state: String,
}

impl ExternalLogoutRedirect {
    /// 返回经 Provider discovery 固定的退出跳转地址。
    pub fn url(&self) -> &url::Url {
        &self.url
    }
    /// 返回需由退出后回调一次性提交的 state。
    pub fn state(&self) -> &str {
        &self.state
    }
}

impl std::fmt::Debug for ExternalLogoutRedirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalLogoutRedirect")
            .field("url", &"[REDACTED]")
            .field("state", &"[REDACTED]")
            .finish()
    }
}

/// 后通道退出的幂等处理结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackchannelLogoutOutcome {
    /// 新通知已处理，返回撤销的应用关联数。
    Processed(u64),
    /// 此 `jti` 已由共享数据库处理。
    AlreadyProcessed,
}

impl ExternalLoginResult {
    /// 返回本地凭据或 MFA challenge。
    pub fn attempt(&self) -> &LoginAttempt {
        &self.attempt
    }

    /// 取出认证结果与关联句柄。
    pub fn into_parts(self) -> (LoginAttempt, ExternalLoginHandle) {
        (self.attempt, self.handle)
    }
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
    encryptor: ContextualEncrypter,
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
        encryptor: ContextualEncrypter,
        providers: impl IntoIterator<Item = Arc<dyn ExternalIdentityProvider>>,
    ) -> Result<Self> {
        auth.require_store(&db)?;
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
        Ok(self
            .complete_internal(
                provider_name,
                callback_state,
                code,
                browser_binding,
                request_source,
                false,
            )
            .await?
            .attempt)
    }

    /// 完成外部登录并创建待关联的短时句柄，供调用方绑定 refresh 与 Session。
    pub async fn complete_with_handle(
        &self,
        provider_name: &str,
        callback_state: &str,
        code: &str,
        browser_binding: &str,
        request_source: &str,
    ) -> Result<ExternalLoginResult> {
        self.complete_internal(
            provider_name,
            callback_state,
            code,
            browser_binding,
            request_source,
            true,
        )
        .await
    }

    #[allow(
        clippy::too_many_lines,
        reason = "回调消费、身份签发和关联记录须共用事务边界"
    )]
    async fn complete_internal(
        &self,
        provider_name: &str,
        callback_state: &str,
        code: &str,
        browser_binding: &str,
        request_source: &str,
        track_association: bool,
    ) -> Result<ExternalLoginResult> {
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
        let handle = ExternalLoginHandle(random_secret()?);
        let handle_hash = digest(handle.expose());
        let session_hash = identity.session_id().map(digest);
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
                let handle_hash = handle_hash.clone();
                let session_hash = session_hash.clone();
                Box::pin(async move {
                    consume_request(transaction, &db, &provider_name, &state_hash).await?;
                    let sid_revoked = if let Some(session_id) = identity.session_id() {
                        lock_external_logout_scope(
                            transaction,
                            &db,
                            &provider_name,
                            identity.issuer(),
                            &digest(session_id),
                            None,
                        )
                        .await?
                    } else {
                        false
                    };
                    let subject_revoked = lock_external_logout_scope(
                        transaction,
                        &db,
                        &provider_name,
                        identity.issuer(),
                        &digest(identity.subject()),
                        None,
                    )
                    .await?;
                    if sid_revoked || subject_revoked {
                        return Ok(None);
                    }
                    let credentials = auth
                        .complete_external_identity_in_transaction(
                            transaction,
                            &provider_name,
                            &identity,
                            password_hash.as_ref(),
                            &request_source,
                        )
                        .await?;
                    let user_id = credentials.user_id();
                    let refresh_family = match &credentials {
                        LoginAttempt::Authenticated(credentials) => Some(
                            refresh_family_in_transaction(
                                transaction,
                                &db,
                                credentials.user_id(),
                                credentials.refresh_token().expose(),
                            )
                            .await?,
                        ),
                        #[cfg(feature = "auth-fortify")]
                        LoginAttempt::TwoFactorRequired(_) => None,
                    };
                    if track_association {
                        #[cfg(feature = "auth-fortify")]
                        let challenge_hash = match &credentials {
                            LoginAttempt::TwoFactorRequired(challenge) => {
                                Some(digest(challenge.expose()))
                            }
                            LoginAttempt::Authenticated(_) => None,
                        };
                        #[cfg(not(feature = "auth-fortify"))]
                        let challenge_hash: Option<String> = None;
                        insert_external_session_in_transaction(
                            transaction,
                            &db,
                            &provider_name,
                            identity.issuer(),
                            &digest(identity.subject()),
                            session_hash.as_deref(),
                            user_id,
                            &handle_hash,
                            refresh_family.as_deref(),
                            challenge_hash.as_deref(),
                            match &credentials {
                                LoginAttempt::Authenticated(credentials) => {
                                    credentials.refresh_expires_at()
                                }
                                #[cfg(feature = "auth-fortify")]
                                LoginAttempt::TwoFactorRequired(challenge) => {
                                    challenge.expires_at()
                                }
                            },
                        )
                        .await?;
                    }
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
                    if let Some(session_id) = identity.session_id() {
                        release_external_logout_scope(
                            transaction,
                            &db,
                            &provider_name,
                            identity.issuer(),
                            &digest(session_id),
                        )
                        .await?;
                    }
                    release_external_logout_scope(
                        transaction,
                        &db,
                        &provider_name,
                        identity.issuer(),
                        &digest(identity.subject()),
                    )
                    .await?;
                    Ok(Some(credentials))
                })
            })
            .await
            .and_then(|attempt| attempt.ok_or(Error::Unauthorized))
            .map(|attempt| ExternalLoginResult { attempt, handle })
    }

    /// 将已登录账户的共享 Session ID 加密关联到外部登录句柄。
    pub async fn attach_session(
        &self,
        user_id: i64,
        handle: &ExternalLoginHandle,
        session: &Session,
    ) -> Result<()> {
        #[cfg(feature = "auth-fortify")]
        if let Some(authority) = self.auth.attachment_authority() {
            let prepared = authority
                .prepare_session_context_attachment(user_id, session)
                .await?;
            return self
                .attach_verified_session(handle, session, authority, prepared)
                .await;
        }
        session.require_authenticated_storage()?;
        if user_id <= 0
            || session
                .get_typed::<i64>(crate::auth::database::AUTH_USER_ID_KEY)
                .map_err(Error::internal)?
                != Some(user_id)
        {
            return Err(Error::Unauthorized);
        }
        self.attach_session_record(user_id, handle, session).await
    }

    /// 关联已通过指定具名 Guard 认证的持久 Session，不接受 once 或旧身份键旁路。
    pub async fn attach_session_with_guard(
        &self,
        user_id: i64,
        handle: &ExternalLoginHandle,
        session: &Session,
        guard: &dyn crate::auth::StatefulGuard,
    ) -> Result<()> {
        #[cfg(feature = "auth-fortify")]
        if let Some(authority) = self.auth.attachment_authority() {
            let prepared = authority
                .prepare_guard_attachment(user_id, session, guard)
                .await?;
            prepared.recheck_guard(session, guard).await?;
            return self
                .attach_verified_session(handle, session, authority, prepared)
                .await;
        }
        super::session::require_account_provider(&self.db, guard)?;
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        super::session::guard_session_key(session, guard, &serde_json::Value::from(user_id))
            .await?;
        self.attach_session_record(user_id, handle, session).await
    }

    #[cfg(feature = "auth-fortify")]
    async fn attach_verified_session(
        &self,
        handle: &ExternalLoginHandle,
        session: &Session,
        authority: &super::fortify::CredentialAuthority,
        prepared: super::fortify::PreparedSessionAttachment,
    ) -> Result<()> {
        prepared.recheck_session(session)?;
        let id = session.id()?.to_string();
        let hash = digest(handle.expose());
        let encrypted = self
            .encryptor
            .encrypt_with_context(id.as_bytes(), hash.as_bytes())?;
        let db = self.db.clone();
        let authority = authority.clone();
        let session = session.clone();
        let prepared = Arc::new(prepared);
        let final_prepared = prepared.clone();
        let final_authority = authority.clone();
        let final_db = db.clone();
        let final_session = session.clone();
        let checkpoint = Arc::new(std::sync::OnceLock::<ExternalAttachmentCheck>::new());
        let captured = checkpoint.clone();
        self.db.transaction_for_update_checked(move |tx| {
            Box::pin(async move {
                authority.validate_attachment_in_transaction(tx, &prepared).await?;
                let (challenge, association_expiry) = lock_active_attachment(tx, &db, prepared.user_id(), &hash, false).await?;
                if let Some(challenge) = &challenge {
                    revoke_external_two_factor_challenge(tx, &db, challenge, prepared.user_id(), Some("auth_factor_challenges")).await?;
                }
                prepared.recheck_session(&session)?;
                let table = db.quoted_table_name("auth_external_sessions")?;
                let backend = tx.get_database_backend();
                let sql = match backend {
                    DbBackend::Postgres => format!("UPDATE {table} SET encrypted_session_id=$1, challenge_hash=NULL WHERE id_hash=$2 AND user_id=$3 AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP"),
                    DbBackend::MySql => format!("UPDATE {table} SET encrypted_session_id=?, challenge_hash=NULL WHERE id_hash=? AND user_id=? AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP(6)"),
                    _ => return Err(oidc_error()),
                };
                let changed = tx.execute_raw(Statement::from_sql_and_values(backend, sql,
                    [encrypted.into(), hash.clone().into(), prepared.user_id().into()])).await.map_err(Error::internal)?;
                if changed.rows_affected()!=1 { return Err(Error::Unauthorized); }
                captured.set(ExternalAttachmentCheck::capture(tx, &db, &hash, prepared.user_id(), association_expiry, challenge).await?).map_err(|_| Error::Unauthorized)?;
                prepared.recheck_session(&session)?;
                Ok(())
            })
        }, move |tx| Box::pin(async move {
            final_authority.validate_attachment_in_transaction(tx, &final_prepared).await?;
            checkpoint.get().ok_or(Error::Unauthorized)?.validate(tx, &final_db).await?;
            final_prepared.recheck_session(&final_session)
        })).await
    }

    async fn attach_session_record(
        &self,
        user_id: i64,
        handle: &ExternalLoginHandle,
        session: &Session,
    ) -> Result<()> {
        let id = session.id()?.to_string();
        let handle_hash = digest(handle.expose());
        let encrypted = self
            .encryptor
            .encrypt_with_context(id.as_bytes(), handle_hash.as_bytes())?;
        let table = self.db.quoted_table_name("auth_external_sessions")?;
        let backend = self.db.backend()?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "UPDATE {table} SET encrypted_session_id = $1 WHERE id_hash = $2 AND user_id = $3 AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP"
            ),
            DbBackend::MySql => format!(
                "UPDATE {table} SET encrypted_session_id = ? WHERE id_hash = ? AND user_id = ? AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP(6)"
            ),
            _ => return Err(oidc_error()),
        };
        let result = self
            .db
            .connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [encrypted.into(), handle_hash.into(), user_id.into()],
            ))
            .await
            .map_err(Error::internal)?;
        if result.rows_affected() != 1 {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// 在外部登录的 MFA challenge 完成后，将新签发的 refresh 家族绑定到句柄。
    pub async fn attach_refresh_credentials(
        &self,
        user_id: i64,
        handle: &ExternalLoginHandle,
        refresh_token: &str,
    ) -> Result<()> {
        if user_id <= 0 || refresh_token.is_empty() || refresh_token.len() > 512 {
            return Err(Error::Unauthorized);
        }
        #[cfg(feature = "auth-fortify")]
        if self.auth.attachment_authority().is_some() {
            let this = self.clone();
            let handle_hash = digest(handle.expose());
            let raw = Arc::new(refresh_token.to_owned());
            let final_raw = raw.clone();
            let final_this = this.clone();
            let checkpoint = Arc::new(std::sync::OnceLock::<ExternalAttachmentCheck>::new());
            let captured = checkpoint.clone();
            return self.db.transaction_for_update_checked(move |tx| {
                Box::pin(async move {
                    lock_external_logout_users(tx, &this.db, [user_id]).await?;
                    let (challenge, association_expiry)=lock_active_attachment(tx, &this.db, user_id, &handle_hash, true).await?;
                    let (family, expiry) = this.auth.authorize_refresh_attachment(tx, user_id, &raw).await?;
                    if let Some(challenge)=&challenge {
                        revoke_external_two_factor_challenge(tx, &this.db, challenge, user_id, Some("auth_factor_challenges")).await?;
                    }
                    let table = this.db.quoted_table_name("auth_external_sessions")?;
                    let backend = tx.get_database_backend();
                    let sql = match backend {
                        DbBackend::Postgres => format!("UPDATE {table} SET refresh_family_id=$1, expires_at=$2, challenge_hash=NULL WHERE id_hash=$3 AND user_id=$4 AND refresh_family_id IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP"),
                        DbBackend::MySql => format!("UPDATE {table} SET refresh_family_id=?, expires_at=?, challenge_hash=NULL WHERE id_hash=? AND user_id=? AND refresh_family_id IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP(6)"),
                        _ => return Err(oidc_error()),
                    };
                    let changed=tx.execute_raw(Statement::from_sql_and_values(backend, sql,
                        [family.into(), expiry.into(), handle_hash.clone().into(), user_id.into()])).await.map_err(Error::internal)?;
                    if changed.rows_affected()!=1 || expiry<=Utc::now() || association_expiry<=Utc::now() { return Err(Error::Unauthorized); }
                    captured.set(ExternalAttachmentCheck::capture(tx, &this.db, &handle_hash, user_id, association_expiry, challenge).await?).map_err(|_| Error::Unauthorized)?;
                    Ok(())
                })
            }, move |tx| Box::pin(async move {
                let check = checkpoint.get().ok_or(Error::Unauthorized)?;
                let (family, expiry) = final_this.auth.authorize_refresh_attachment(tx, user_id, &final_raw).await?;
                if check.family.as_deref()!=Some(family.as_str()) || check.expiry!=expiry { return Err(Error::Unauthorized); }
                check.validate(tx, &final_this.db).await
            })).await;
        }

        let handle_hash = digest(handle.expose());
        let db = self.db.clone();
        let token = refresh_token.to_owned();
        self.db.transaction(|transaction| {
            let db = db.clone();
            let token = token.clone();
            let handle_hash = handle_hash.clone();
            Box::pin(async move {
                // 公开attach入口同样先占有账户；此事务内不跨Guard/Session的await。
                lock_external_logout_users(transaction, &db, [user_id]).await?;
                let family_id = refresh_family_in_transaction(transaction, &db, user_id, &token).await?;
                let association = db.quoted_table_name("auth_external_sessions")?;
                let refresh = db.quoted_table_name("auth_refresh_tokens")?;
                let backend = transaction.get_database_backend();
                let sql = match backend {
                    DbBackend::Postgres => format!("UPDATE {association} SET refresh_family_id = $1, expires_at = (SELECT MAX(expires_at) FROM {refresh} WHERE family_id = $1) WHERE id_hash = $2 AND user_id = $3 AND refresh_family_id IS NULL AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP"),
                    DbBackend::MySql => format!("UPDATE {association} SET refresh_family_id = ?, expires_at = (SELECT MAX(expires_at) FROM {refresh} WHERE family_id = ?) WHERE id_hash = ? AND user_id = ? AND refresh_family_id IS NULL AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP(6)"),
                    _ => return Err(oidc_error()),
                };
                let values = if backend == DbBackend::Postgres {
                    vec![family_id.clone().into(), handle_hash.into(), user_id.into()]
                } else {
                    vec![family_id.clone().into(), family_id.into(), handle_hash.into(), user_id.into()]
                };
                let changed = transaction.execute_raw(Statement::from_sql_and_values(backend, &sql, values)).await.map_err(Error::internal)?;
                if changed.rows_affected() != 1 { return Err(Error::Unauthorized); }
                Ok(())
            })
        }).await
    }

    /// 验证并处理 `IdP` 后通道退出通知；重放检查和凭据撤销由共享数据库串行化。
    #[allow(
        clippy::too_many_lines,
        reason = "重放保护、共享关联撤销及 Session 清理必须使用同一退出语义"
    )]
    pub async fn process_backchannel_logout(
        &self,
        provider_name: &str,
        compact_token: &str,
        sessions: &SessionStoreHandle,
    ) -> Result<BackchannelLogoutOutcome> {
        if compact_token.is_empty() || compact_token.len() > 16 * 1024 {
            return Err(oidc_error());
        }
        let provider = self.provider(provider_name)?;
        let token = provider.verify_logout_token(compact_token).await?;
        if token.issuer() != provider.issuer() {
            return Err(oidc_error());
        }
        let db = self.db.clone();
        let audit = self.audit.clone();
        let provider_name = provider_name.to_owned();
        let issuer = token.issuer().to_owned();
        let jti_hash = digest(token.token_id());
        let session_hash = token.session_id().map(digest);
        let subject_hash = token.subject().map(digest);
        let expired_at = Utc::now() + ChronoDuration::days(1);
        #[cfg(feature = "auth-fortify")]
        let modern_challenge = self.auth.external_challenge_table();
        let pending_sessions = self.db.transaction(|transaction| {
            let db = db.clone();
            let audit = audit.clone();
            let provider_name = provider_name.clone();
            let issuer = issuer.clone();
            let jti_hash = jti_hash.clone();
            let session_hash = session_hash.clone();
            let subject_hash = subject_hash.clone();
            Box::pin(async move {
                let replay = db.quoted_table_name("auth_external_logout_tokens")?;
                let backend = transaction.get_database_backend();
                let insert = match backend {
                    DbBackend::Postgres => format!("INSERT INTO {replay} (provider, issuer, token_id_hash, expires_at, created_at) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP) ON CONFLICT (provider, issuer, token_id_hash) DO NOTHING"),
                    DbBackend::MySql => format!("INSERT IGNORE INTO {replay} (provider, issuer, token_id_hash, expires_at, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP(6))"),
                    _ => return Err(oidc_error()),
                };
                let inserted = transaction.execute_raw(Statement::from_sql_and_values(
                    backend,
                    &insert,
                    [provider_name.clone().into(), issuer.clone().into(), jti_hash.into(), expired_at.into()],
                )).await.map_err(Error::internal)?;
                let is_new = inserted.rows_affected() != 0;
                let scope_hash = session_hash
                    .as_deref()
                    .or(subject_hash.as_deref())
                    .ok_or_else(oidc_error)?;
                lock_external_logout_scope(
                    transaction,
                    &db,
                    &provider_name,
                    &issuer,
                    scope_hash,
                    Some(Utc::now() + ChronoDuration::minutes(10)),
                )
                .await?;
                let active_clause = if is_new {
                    "revoked_at IS NULL"
                } else {
                    "revoked_at IS NOT NULL AND encrypted_session_id IS NOT NULL AND session_deleted_at IS NULL"
                };
                let table = db.quoted_table_name("auth_external_sessions")?;
                let (lookup, values) = match (backend, session_hash, subject_hash) {
                    (DbBackend::Postgres, Some(sid), _) => (
                        format!("SELECT id_hash, user_id, refresh_family_id, encrypted_session_id, challenge_hash FROM {table} WHERE provider = $1 AND issuer = $2 AND session_hash = $3 AND {active_clause} ORDER BY id_hash FOR UPDATE"),
                        vec![provider_name.clone().into(), issuer.clone().into(), sid.into()],
                    ),
                    (DbBackend::MySql, Some(sid), _) => (
                        format!("SELECT id_hash, user_id, refresh_family_id, encrypted_session_id, challenge_hash FROM {table} WHERE provider = ? AND issuer = ? AND session_hash = ? AND {active_clause} ORDER BY id_hash FOR UPDATE"),
                        vec![provider_name.clone().into(), issuer.clone().into(), sid.into()],
                    ),
                    (DbBackend::Postgres, None, Some(subject)) => (
                        format!("SELECT id_hash, user_id, refresh_family_id, encrypted_session_id, challenge_hash FROM {table} WHERE provider = $1 AND issuer = $2 AND subject_hash = $3 AND {active_clause} ORDER BY id_hash FOR UPDATE"),
                        vec![provider_name.clone().into(), issuer.clone().into(), subject.into()],
                    ),
                    (DbBackend::MySql, None, Some(subject)) => (
                        format!("SELECT id_hash, user_id, refresh_family_id, encrypted_session_id, challenge_hash FROM {table} WHERE provider = ? AND issuer = ? AND subject_hash = ? AND {active_clause} ORDER BY id_hash FOR UPDATE"),
                        vec![provider_name.clone().into(), issuer.clone().into(), subject.into()],
                    ),
                    _ => return Err(oidc_error()),
                };
                let rows = lock_external_logout_associations(transaction, &db, backend, &lookup, values).await?;
                let mut revoked = Vec::with_capacity(rows.len());
                for row in rows {
                    let id_hash: String = row.try_get("", "id_hash").map_err(Error::internal)?;
                    let owner_id: i64 = row.try_get("", "user_id").map_err(Error::internal)?;
                    let family: Option<String> = row.try_get("", "refresh_family_id").map_err(Error::internal)?;
                    let encrypted: Option<String> = row.try_get("", "encrypted_session_id").map_err(Error::internal)?;
                    #[cfg(feature = "auth-fortify")]
                    if is_new
                        && let Some(challenge_hash) = row
                            .try_get::<Option<String>>("", "challenge_hash")
                            .map_err(Error::internal)?
                    {
                        revoke_external_two_factor_challenge(transaction, &db, &challenge_hash, owner_id, modern_challenge).await?;
                    }
                    if is_new && let Some(family) = family {
                        crate::auth::refresh::revoke_family_for_locked_owner_in_transaction(transaction, &db, backend, &family, owner_id).await?;
                    }
                    if is_new {
                        let update = match backend {
                        DbBackend::Postgres => format!("UPDATE {table} SET revoked_at = CURRENT_TIMESTAMP WHERE id_hash = $1 AND revoked_at IS NULL"),
                        DbBackend::MySql => format!("UPDATE {table} SET revoked_at = CURRENT_TIMESTAMP(6) WHERE id_hash = ? AND revoked_at IS NULL"),
                        _ => return Err(oidc_error()),
                        };
                        transaction.execute_raw(Statement::from_sql_and_values(backend, &update, [id_hash.clone().into()])).await.map_err(Error::internal)?;
                    }
                    revoked.push((id_hash, encrypted));
                }
                if is_new {
                    record_external_logout(&audit, transaction, &provider_name, None, "auth.external.backchannel_logout").await?;
                }
                Ok((is_new, revoked))
            })
        }).await?;
        for (id_hash, encrypted) in &pending_sessions.1 {
            let Some(encrypted) = encrypted else {
                continue;
            };
            delete_external_session(&self.encryptor, &self.db, sessions, id_hash, encrypted)
                .await?;
        }
        if !pending_sessions.0 {
            return Ok(BackchannelLogoutOutcome::AlreadyProcessed);
        }
        Ok(BackchannelLogoutOutcome::Processed(
            pending_sessions.1.len() as u64,
        ))
    }

    /// 先撤销指定外部登录关联，再返回固定 `IdP` logout endpoint。
    #[allow(
        clippy::too_many_lines,
        reason = "本地撤销、Session 清理与一次性 RP logout state 必须按固定事务顺序执行"
    )]
    pub async fn begin_logout(
        &self,
        user_id: i64,
        handle: &ExternalLoginHandle,
        post_logout_redirect_uri: &str,
        sessions: &SessionStoreHandle,
    ) -> Result<Option<ExternalLogoutRedirect>> {
        if user_id <= 0 || post_logout_redirect_uri.len() > 2048 {
            return Err(Error::Unauthorized);
        }
        let handle_hash = digest(handle.expose());
        #[cfg(feature = "auth-fortify")]
        let modern_challenge = self.auth.external_challenge_table();
        let db = self.db.clone();
        let audit = self.audit.clone();
        let redirect = post_logout_redirect_uri.to_owned();
        let providers = self.providers.clone();
        let data = self.db.transaction(|transaction| {
            let db = db.clone();
            let audit = audit.clone();
            let handle_hash = handle_hash.clone();
            let redirect = redirect.clone();
            let providers = providers.clone();
            Box::pin(async move {
                let table = db.quoted_table_name("auth_external_sessions")?;
                let backend = transaction.get_database_backend();
                let identity_sql = match backend {
                    DbBackend::Postgres => format!("SELECT provider, issuer, session_hash, encrypted_session_id, CASE WHEN revoked_at IS NULL THEN 0 ELSE 1 END AS revoked_flag, CASE WHEN session_deleted_at IS NULL THEN 0 ELSE 1 END AS session_deleted_flag FROM {table} WHERE id_hash = $1 AND user_id = $2"),
                    DbBackend::MySql => format!("SELECT provider, issuer, session_hash, encrypted_session_id, CASE WHEN revoked_at IS NULL THEN 0 ELSE 1 END AS revoked_flag, CASE WHEN session_deleted_at IS NULL THEN 0 ELSE 1 END AS session_deleted_flag FROM {table} WHERE id_hash = ? AND user_id = ?"),
                    _ => return Err(oidc_error()),
                };
                let identity_row = transaction.query_one_raw(Statement::from_sql_and_values(backend, &identity_sql, [handle_hash.clone().into(), user_id.into()])).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
                let provider_name: String = identity_row.try_get("", "provider").map_err(Error::internal)?;
                let revoked: i32 = identity_row.try_get("", "revoked_flag").map_err(Error::internal)?;
                if revoked != 0 {
                    let session_deleted: i32 = identity_row.try_get("", "session_deleted_flag").map_err(Error::internal)?;
                    let pending = if session_deleted == 0 {
                        identity_row.try_get::<Option<String>>("", "encrypted_session_id").map_err(Error::internal)?
                    } else {
                        None
                    };
                    return Ok((None, pending));
                }
                let provider = providers.get(&provider_name).ok_or_else(oidc_error)?;
                if !redirect.is_empty()
                    && provider.post_logout_redirect_uri().as_deref() != Some(redirect.as_str())
                {
                    return Err(Error::Unauthorized);
                }
                let issuer: String = identity_row.try_get("", "issuer").map_err(Error::internal)?;
                let session_hash: Option<String> = identity_row.try_get("", "session_hash").map_err(Error::internal)?;
                if let Some(session_hash) = session_hash.as_deref() {
                    lock_external_logout_scope(
                        transaction,
                        &db,
                        &provider_name,
                        &issuer,
                        session_hash,
                        Some(Utc::now() + ChronoDuration::minutes(10)),
                    )
                    .await?;
                }
                lock_external_logout_users(transaction, &db, [user_id]).await?;
                let sql = match backend {
                    DbBackend::Postgres => format!("SELECT user_id, provider, issuer, session_hash, refresh_family_id, encrypted_session_id, challenge_hash FROM {table} WHERE id_hash = $1 AND user_id = $2 AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP FOR UPDATE"),
                    DbBackend::MySql => format!("SELECT user_id, provider, issuer, session_hash, refresh_family_id, encrypted_session_id, challenge_hash FROM {table} WHERE id_hash = ? AND user_id = ? AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP(6) FOR UPDATE"),
                    _ => return Err(oidc_error()),
                };
                let row = transaction.query_one_raw(Statement::from_sql_and_values(backend, &sql, [handle_hash.clone().into(), user_id.into()])).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
                let owner_id: i64 = row.try_get("", "user_id").map_err(Error::internal)?;
                if owner_id != user_id
                    || row.try_get::<String>("", "provider").map_err(Error::internal)? != provider_name
                    || row.try_get::<String>("", "issuer").map_err(Error::internal)? != issuer
                    || row.try_get::<Option<String>>("", "session_hash").map_err(Error::internal)? != session_hash {
                    return Err(Error::Unauthorized);
                }
                let family: Option<String> = row.try_get("", "refresh_family_id").map_err(Error::internal)?;
                let encrypted_session: Option<String> = row.try_get("", "encrypted_session_id").map_err(Error::internal)?;
                #[cfg(feature = "auth-fortify")]
                if let Some(challenge_hash) = row.try_get::<Option<String>>("", "challenge_hash").map_err(Error::internal)? {
                    revoke_external_two_factor_challenge(transaction, &db, &challenge_hash, owner_id, modern_challenge).await?;
                }
                if let Some(family) = family {
                    crate::auth::refresh::revoke_family_for_locked_owner_in_transaction(transaction, &db, backend, &family, owner_id).await?;
                }
                let update = match backend {
                    DbBackend::Postgres => format!("UPDATE {table} SET revoked_at = CURRENT_TIMESTAMP WHERE id_hash = $1 AND revoked_at IS NULL"),
                    DbBackend::MySql => format!("UPDATE {table} SET revoked_at = CURRENT_TIMESTAMP(6) WHERE id_hash = ? AND revoked_at IS NULL"),
                    _ => return Err(oidc_error()),
                };
                transaction.execute_raw(Statement::from_sql_and_values(backend, &update, [handle_hash.clone().into()])).await.map_err(Error::internal)?;
                record_external_logout(&audit, transaction, &provider_name, Some(user_id), "auth.external.logout").await?;
                let request_state = random_secret()?;
                let request_state_hash = digest(&request_state);
                let state = if provider.end_session_endpoint().is_some() && !redirect.is_empty() {
                    Some((request_state, request_state_hash))
                } else {
                    None
                };
                if let Some((_, state_hash)) = &state {
                    let requests = db.quoted_table_name("auth_external_logout_requests")?;
                    let insert = match backend {
                        DbBackend::Postgres => format!("INSERT INTO {requests} (state_hash, provider, association_id_hash, post_logout_redirect_uri, expires_at, created_at) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP + INTERVAL '10 minutes', CURRENT_TIMESTAMP)"),
                        DbBackend::MySql => format!("INSERT INTO {requests} (state_hash, provider, association_id_hash, post_logout_redirect_uri, expires_at, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP(6) + INTERVAL 10 MINUTE, CURRENT_TIMESTAMP(6))"),
                        _ => return Err(oidc_error()),
                    };
                    transaction.execute_raw(Statement::from_sql_and_values(backend, &insert, [state_hash.clone().into(), provider_name.clone().into(), handle_hash.into(), redirect.clone().into()])).await.map_err(Error::internal)?;
                }
                Ok((Some((provider_name, encrypted_session, state)), None))
            })
        }).await?;
        if let Some(encrypted) = data.1 {
            delete_external_session(
                &self.encryptor,
                &self.db,
                sessions,
                &handle_hash,
                &encrypted,
            )
            .await?;
            return Ok(None);
        }
        let Some((provider_name, encrypted_session, state)) = data.0 else {
            return Ok(None);
        };
        if let Some(encrypted) = encrypted_session {
            delete_external_session(
                &self.encryptor,
                &self.db,
                sessions,
                &handle_hash,
                &encrypted,
            )
            .await?;
        }
        let Some((state, _state_hash)) = state else {
            return Ok(None);
        };
        let provider = self.provider(&provider_name)?;
        let Some(mut url) = provider.end_session_endpoint() else {
            return Ok(None);
        };
        if provider.post_logout_redirect_uri().as_deref() != Some(post_logout_redirect_uri) {
            return Err(Error::Unauthorized);
        }
        let client_id = provider.client_id().ok_or_else(oidc_error)?;
        url.query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("post_logout_redirect_uri", post_logout_redirect_uri)
            .append_pair("state", &state);
        Ok(Some(ExternalLogoutRedirect { url, state }))
    }

    /// 一次性消费已登记退出 state，并返回服务端保存的固定回调 URI。
    pub async fn complete_logout(&self, provider_name: &str, state: &str) -> Result<url::Url> {
        if state.is_empty() || state.len() > 256 {
            return Err(oidc_error());
        }
        let table = self.db.quoted_table_name("auth_external_logout_requests")?;
        let backend = self.db.backend()?;
        self.db.transaction(|transaction| {
            let table = table.clone();
            let state_hash = digest(state);
            let provider_name = provider_name.to_owned();
            Box::pin(async move {
                let (select, values) = match backend {
                    DbBackend::Postgres => (format!("SELECT post_logout_redirect_uri FROM {table} WHERE state_hash = $1 AND provider = $2 AND expires_at > CURRENT_TIMESTAMP FOR UPDATE"), vec![state_hash.clone().into(), provider_name.clone().into()]),
                    DbBackend::MySql => (format!("SELECT post_logout_redirect_uri FROM {table} WHERE state_hash = ? AND provider = ? AND expires_at > CURRENT_TIMESTAMP(6) FOR UPDATE"), vec![state_hash.clone().into(), provider_name.clone().into()]),
                    _ => return Err(oidc_error()),
                };
                let row = transaction.query_one_raw(Statement::from_sql_and_values(backend, &select, values)).await.map_err(Error::internal)?.ok_or_else(oidc_error)?;
                let redirect: String = row.try_get("", "post_logout_redirect_uri").map_err(Error::internal)?;
                let delete = match backend {
                    DbBackend::Postgres => format!("DELETE FROM {table} WHERE state_hash = $1 AND provider = $2"),
                    DbBackend::MySql => format!("DELETE FROM {table} WHERE state_hash = ? AND provider = ?"),
                    _ => return Err(oidc_error()),
                };
                transaction.execute_raw(Statement::from_sql_and_values(backend, &delete, [state_hash.into(), provider_name.into()])).await.map_err(Error::internal)?;
                url::Url::parse(&redirect).map_err(|_| oidc_error())
            })
        }).await
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

async fn refresh_family_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    refresh_token: &str,
) -> Result<String> {
    let table = db.quoted_table_name("auth_refresh_tokens")?;
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => {
            format!("SELECT family_id FROM {table} WHERE token_hash = $1 AND user_id = $2")
        }
        DbBackend::MySql => {
            format!("SELECT family_id FROM {table} WHERE token_hash = ? AND user_id = ?")
        }
        _ => return Err(oidc_error()),
    };
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [digest(refresh_token).into(), user_id.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(oidc_error)?;
    row.try_get::<String>("", "family_id")
        .map_err(Error::internal)
}

#[allow(
    clippy::too_many_arguments,
    reason = "登录关联记录必须与身份和 refresh 家族在同一事务提交"
)]
async fn insert_external_session_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    provider: &str,
    issuer: &str,
    subject_hash: &str,
    session_hash: Option<&str>,
    user_id: i64,
    id_hash: &str,
    family_id: Option<&str>,
    challenge_hash: Option<&str>,
    expires_at: chrono::DateTime<Utc>,
) -> Result<()> {
    let table = db.quoted_table_name("auth_external_sessions")?;
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (id_hash, user_id, provider, issuer, subject_hash, session_hash, refresh_family_id, challenge_hash, encrypted_session_id, expires_at, revoked_at, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, NULL, CURRENT_TIMESTAMP)"
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {table} (id_hash, user_id, provider, issuer, subject_hash, session_hash, refresh_family_id, challenge_hash, encrypted_session_id, expires_at, revoked_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, NULL, CURRENT_TIMESTAMP(6))"
        ),
        _ => return Err(oidc_error()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [
                id_hash.to_owned().into(),
                user_id.into(),
                provider.to_owned().into(),
                issuer.to_owned().into(),
                subject_hash.to_owned().into(),
                session_hash.map(str::to_owned).into(),
                family_id.map(str::to_owned).into(),
                challenge_hash.map(str::to_owned).into(),
                expires_at.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

async fn record_external_logout(
    audit: &DatabaseAuditSink,
    transaction: &DatabaseTransaction,
    provider: &str,
    user_id: Option<i64>,
    action: &str,
) -> Result<()> {
    let event = AuditEvent::new(
        action,
        AuditSubject::new("external_provider", provider)?,
        AuditOutcome::Succeeded,
    )?;
    let event = if let Some(user_id) = user_id {
        event.actor(AuditSubject::new("user", user_id.to_string())?)
    } else {
        event
    };
    let record = Audit::new(audit.clone(), AuditPolicy::default()).prepare(event)?;
    audit.record_in_transaction(transaction, &record).await
}

async fn delete_external_session(
    encryptor: &ContextualEncrypter,
    db: &Db,
    sessions: &SessionStoreHandle,
    association_hash: &str,
    encrypted_session_id: &str,
) -> Result<()> {
    let session_id =
        encryptor.decrypt_with_context(encrypted_session_id, association_hash.as_bytes())?;
    let session_id = String::from_utf8(session_id).map_err(Error::internal)?;
    // 原生数据库 driver 允许当前 DELETE，仍不能阻止并发 INSERT 重建记录。
    // 仅服务端已加密关联的ID使用底层handler；客户端快照仍明确拒绝。
    if sessions.guarantees() == crate::session::SessionStoreGuarantees::NativeDatabase {
        sessions
            .store()
            .delete(&crate::session::SessionId::parse(&session_id)?)
            .await?;
    } else {
        sessions.delete(&session_id).await?;
    }
    let table = db.quoted_table_name("auth_external_sessions")?;
    let backend = db.backend()?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "UPDATE {table} SET session_deleted_at = CURRENT_TIMESTAMP WHERE id_hash = $1 AND session_deleted_at IS NULL"
        ),
        DbBackend::MySql => format!(
            "UPDATE {table} SET session_deleted_at = CURRENT_TIMESTAMP(6) WHERE id_hash = ? AND session_deleted_at IS NULL"
        ),
        _ => return Err(oidc_error()),
    };
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [association_hash.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

#[cfg(feature = "auth-fortify")]
async fn revoke_external_two_factor_challenge(
    transaction: &DatabaseTransaction,
    db: &Db,
    challenge_hash: &str,
    owner_id: i64,
    logical_table: Option<&str>,
) -> Result<()> {
    let Some(logical_table) = logical_table else {
        return Ok(());
    };
    let table = db.quoted_table_name(logical_table)?;
    let backend = transaction.get_database_backend();
    let owner_sql = match backend {
        DbBackend::Postgres => {
            format!("SELECT user_id FROM {table} WHERE token_hash = $1 FOR UPDATE")
        }
        DbBackend::MySql => {
            format!("SELECT user_id FROM {table} WHERE token_hash = ? FOR UPDATE")
        }
        _ => return Err(oidc_error()),
    };
    if let Some(owner) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            owner_sql,
            [challenge_hash.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?
        && owner
            .try_get::<i64>("", "user_id")
            .map_err(Error::internal)?
            != owner_id
    {
        return Err(Error::Unauthorized);
    }
    let sql = match backend {
        DbBackend::Postgres => {
            format!("DELETE FROM {table} WHERE token_hash = $1 AND user_id = $2")
        }
        DbBackend::MySql => format!("DELETE FROM {table} WHERE token_hash = ? AND user_id = ?"),
        _ => return Err(oidc_error()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [challenge_hash.to_owned().into(), owner_id.into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

#[cfg(all(feature = "auth-oidc", feature = "auth-fortify"))]
pub(crate) async fn lock_external_two_factor_challenge(
    transaction: &DatabaseTransaction,
    db: &Db,
    challenge: &str,
) -> Result<Option<(String, i64)>> {
    let table = db.quoted_table_name("auth_external_sessions")?;
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT id_hash, user_id, revoked_at, expires_at FROM {table} WHERE challenge_hash = $1 FOR UPDATE"
        ),
        DbBackend::MySql => format!(
            "SELECT id_hash, user_id, revoked_at, expires_at FROM {table} WHERE challenge_hash = ? FOR UPDATE"
        ),
        DbBackend::Sqlite => {
            format!(
                "SELECT id_hash, user_id, revoked_at, expires_at FROM {table} WHERE challenge_hash = ?"
            )
        }
        _ => return Err(oidc_error()),
    };
    let Some(row) = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [digest(challenge).into()],
        ))
        .await
        .map_err(Error::internal)?
    else {
        return Ok(None);
    };
    if backend == DbBackend::Sqlite {
        // 本地挑战没有外部关联可以继续；真实关联不得因尚未支持SQLite OIDC而被当成本地挑战。
        return Err(Error::custom_with_status(
            crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "unsupported_oidc_database",
            "SQLite外部身份关联尚未完成，不能消费为本地MFA挑战",
        ));
    }
    let revoked_at: Option<chrono::DateTime<Utc>> =
        row.try_get("", "revoked_at").map_err(Error::internal)?;
    let expiry: chrono::DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
    if revoked_at.is_some() || expiry <= Utc::now() {
        return Err(Error::Unauthorized);
    }
    Ok(Some((
        row.try_get("", "id_hash").map_err(Error::internal)?,
        row.try_get("", "user_id").map_err(Error::internal)?,
    )))
}

#[cfg(feature = "auth-fortify")]
#[derive(PartialEq, Eq)]
pub(crate) struct ExternalChallengeBinding {
    id_hash: String,
    user_id: i64,
    provider: String,
    issuer: String,
    subject_hash: String,
    session_hash: Option<String>,
}

#[cfg(feature = "auth-fortify")]
impl ExternalChallengeBinding {
    // 调用方已按 users→association 顺序取得这条关联的排他锁。
    async fn load(
        tx: &DatabaseTransaction,
        db: &Db,
        id_hash: &str,
        user_id: i64,
    ) -> Result<(Self, Option<String>, Option<String>, chrono::DateTime<Utc>)> {
        let backend = tx.get_database_backend();
        let (id, owner) = match backend {
            DbBackend::Postgres => ("$1", "$2"),
            DbBackend::MySql => ("?", "?"),
            _ => return Err(oidc_error()),
        };
        let table = db.quoted_table_name("auth_external_sessions")?;
        let row = tx.query_one_raw(Statement::from_sql_and_values(backend,
            format!("SELECT id_hash,user_id,provider,issuer,subject_hash,session_hash,refresh_family_id,challenge_hash,expires_at FROM {table} WHERE id_hash={id} AND user_id={owner} AND revoked_at IS NULL FOR UPDATE"),
            [id_hash.to_owned().into(), user_id.into()])).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
        let binding = Self {
            id_hash: row.try_get("", "id_hash").map_err(Error::internal)?,
            user_id: row.try_get("", "user_id").map_err(Error::internal)?,
            provider: row.try_get("", "provider").map_err(Error::internal)?,
            issuer: row.try_get("", "issuer").map_err(Error::internal)?,
            subject_hash: row.try_get("", "subject_hash").map_err(Error::internal)?,
            session_hash: row.try_get("", "session_hash").map_err(Error::internal)?,
        };
        let expiry = row.try_get("", "expires_at").map_err(Error::internal)?;
        if binding.id_hash != id_hash || binding.user_id != user_id || expiry <= Utc::now() {
            return Err(Error::Unauthorized);
        }
        Ok((
            binding,
            row.try_get("", "challenge_hash").map_err(Error::internal)?,
            row.try_get("", "refresh_family_id")
                .map_err(Error::internal)?,
            expiry,
        ))
    }

    pub(crate) async fn capture(
        tx: &DatabaseTransaction,
        db: &Db,
        association: &(String, i64),
        challenge: &str,
        purpose: &str,
    ) -> Result<(Self, chrono::DateTime<Utc>)> {
        let (binding, hash, family, expiry) =
            Self::load(tx, db, &association.0, association.1).await?;
        if purpose.strip_prefix("external:") != Some(binding.provider.as_str())
            || hash.as_deref() != Some(digest(challenge).as_str())
            || family.is_some()
        {
            return Err(Error::Unauthorized);
        }
        Ok((binding, expiry))
    }

    pub(crate) async fn validate_bound(
        &self,
        tx: &DatabaseTransaction,
        db: &Db,
        family: &str,
        expiry: chrono::DateTime<Utc>,
    ) -> Result<()> {
        let (current, challenge, bound_family, deadline) =
            Self::load(tx, db, &self.id_hash, self.user_id).await?;
        if &current != self
            || challenge.is_some()
            || bound_family.as_deref() != Some(family)
            || deadline != expiry
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}

#[cfg(feature = "auth-fortify")]
struct ExternalAttachmentCheck {
    binding: ExternalChallengeBinding,
    challenge: Option<String>,
    family: Option<String>,
    encrypted_session: Option<String>,
    expiry: chrono::DateTime<Utc>,
    original_expiry: chrono::DateTime<Utc>,
    original_challenge: Option<String>,
}
#[cfg(feature = "auth-fortify")]
impl ExternalAttachmentCheck {
    async fn capture(
        tx: &DatabaseTransaction,
        db: &Db,
        id_hash: &str,
        user_id: i64,
        original_expiry: chrono::DateTime<Utc>,
        original_challenge: Option<String>,
    ) -> Result<Self> {
        let (binding, challenge, family, expiry) =
            ExternalChallengeBinding::load(tx, db, id_hash, user_id).await?;
        let table = db.quoted_table_name("auth_external_sessions")?;
        let backend = tx.get_database_backend();
        let markers = match backend {
            DbBackend::Postgres => ("$1", "$2"),
            DbBackend::MySql => ("?", "?"),
            _ => return Err(oidc_error()),
        };
        let row = tx.query_one_raw(Statement::from_sql_and_values(backend,
            format!("SELECT encrypted_session_id FROM {table} WHERE id_hash={} AND user_id={} AND revoked_at IS NULL", markers.0, markers.1),
            [id_hash.to_owned().into(), user_id.into()])).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
        let encrypted_session = row
            .try_get("", "encrypted_session_id")
            .map_err(Error::internal)?;
        if original_expiry <= Utc::now() || expiry <= Utc::now() {
            return Err(Error::Unauthorized);
        }
        Ok(Self {
            binding,
            challenge,
            family,
            encrypted_session,
            expiry,
            original_expiry,
            original_challenge,
        })
    }
    async fn validate(&self, tx: &DatabaseTransaction, db: &Db) -> Result<()> {
        let current = Self::capture(
            tx,
            db,
            &self.binding.id_hash,
            self.binding.user_id,
            self.original_expiry,
            self.original_challenge.clone(),
        )
        .await?;
        if let Some(hash) = &self.original_challenge {
            let backend = tx.get_database_backend();
            let marker = match backend {
                DbBackend::Postgres => "$1",
                DbBackend::MySql => "?",
                _ => return Err(oidc_error()),
            };
            let table = db.quoted_table_name("auth_factor_challenges")?;
            // 原挑战不得在关联解除后重新成为可独立消费的外部挑战。
            if tx
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    format!("SELECT token_hash FROM {table} WHERE token_hash={marker} FOR UPDATE"),
                    [hash.clone().into()],
                ))
                .await
                .map_err(Error::internal)?
                .is_some()
            {
                return Err(Error::Unauthorized);
            }
        }
        if current.binding != self.binding
            || current.challenge != self.challenge
            || current.family != self.family
            || current.encrypted_session != self.encrypted_session
            || current.expiry != self.expiry
            || self.original_expiry <= Utc::now()
            || self.expiry <= Utc::now()
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}

#[cfg(feature = "auth-fortify")]
pub(crate) async fn bind_external_login_credentials(
    transaction: &DatabaseTransaction,
    db: &Db,
    association: Option<&(String, i64)>,
    challenge: &str,
    credentials: &crate::auth::AuthCredentials,
) -> Result<Option<chrono::DateTime<Utc>>> {
    let Some((_, owner)) = association else {
        return Ok(None);
    };
    if *owner != credentials.user_id() {
        return Err(Error::Unauthorized);
    }
    bind_external_two_factor_credentials(
        transaction,
        db,
        challenge,
        *owner,
        credentials.refresh_token().expose(),
        credentials.refresh_expires_at(),
    )
    .await
    .map(Some)
}

#[cfg(all(feature = "auth-oidc", feature = "auth-fortify"))]
// 调用方已按users→association→factor/refresh持有同一owner；本函数不获取新账户锁。
pub(crate) async fn bind_external_two_factor_credentials(
    transaction: &DatabaseTransaction,
    db: &Db,
    challenge: &str,
    user_id: i64,
    refresh_token: &str,
    expires_at: chrono::DateTime<Utc>,
) -> Result<chrono::DateTime<Utc>> {
    let family_id = refresh_family_in_transaction(transaction, db, user_id, refresh_token).await?;
    let table = db.quoted_table_name("auth_external_sessions")?;
    if expires_at <= Utc::now() {
        return Err(Error::Unauthorized);
    }
    let backend = transaction.get_database_backend();
    let marker = if backend == DbBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    let original = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!("SELECT expires_at FROM {table} WHERE challenge_hash={marker} FOR UPDATE"),
            [digest(challenge).into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    let association_expiry: chrono::DateTime<Utc> = original
        .try_get("", "expires_at")
        .map_err(Error::internal)?;
    if association_expiry <= Utc::now() {
        return Err(Error::Unauthorized);
    }

    let sql = match backend {
        DbBackend::Postgres => format!(
            "UPDATE {table} SET refresh_family_id = $1, challenge_hash = NULL, expires_at = $2 WHERE challenge_hash = $3 AND user_id = $4 AND revoked_at IS NULL AND refresh_family_id IS NULL AND expires_at > CURRENT_TIMESTAMP"
        ),
        DbBackend::MySql => format!(
            "UPDATE {table} SET refresh_family_id = ?, challenge_hash = NULL, expires_at = ? WHERE challenge_hash = ? AND user_id = ? AND revoked_at IS NULL AND refresh_family_id IS NULL AND expires_at > CURRENT_TIMESTAMP(6)"
        ),
        _ => return Err(oidc_error()),
    };
    let values = vec![
        family_id.into(),
        expires_at.into(),
        digest(challenge).into(),
        user_id.into(),
    ];
    let changed = transaction
        .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
        .await
        .map_err(Error::internal)?;
    if changed.rows_affected() != 1 || association_expiry <= Utc::now() || expires_at <= Utc::now()
    {
        return Err(Error::Unauthorized);
    }
    Ok(association_expiry)
}

#[cfg(feature = "auth-fortify")]
async fn lock_active_attachment(
    tx: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    hash: &str,
    require_unbound: bool,
) -> Result<(Option<String>, chrono::DateTime<Utc>)> {
    let table = db.quoted_table_name("auth_external_sessions")?;
    let backend = tx.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT revoked_at,expires_at,refresh_family_id,challenge_hash FROM {table} WHERE id_hash=$1 AND user_id=$2 FOR UPDATE"
        ),
        DbBackend::MySql => format!(
            "SELECT revoked_at,expires_at,refresh_family_id,challenge_hash FROM {table} WHERE id_hash=? AND user_id=? FOR UPDATE"
        ),
        _ => return Err(oidc_error()),
    };
    let row = tx
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [hash.to_owned().into(), user_id.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    let revoked: Option<chrono::DateTime<Utc>> =
        row.try_get("", "revoked_at").map_err(Error::internal)?;
    let expiry: chrono::DateTime<Utc> = row.try_get("", "expires_at").map_err(Error::internal)?;
    let family: Option<String> = row
        .try_get("", "refresh_family_id")
        .map_err(Error::internal)?;
    let challenge: Option<String> = row.try_get("", "challenge_hash").map_err(Error::internal)?;
    if revoked.is_some() || expiry <= Utc::now() || (require_unbound && family.is_some()) {
        return Err(Error::Unauthorized);
    }
    Ok((challenge, expiry))
}

// scope→全部users升序→association→challenge/refresh。预查只是定位，锁定重读才可信。
async fn lock_external_logout_associations(
    transaction: &DatabaseTransaction,
    db: &Db,
    backend: DbBackend,
    lookup: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>> {
    let discovery = lookup.strip_suffix(" FOR UPDATE").ok_or_else(oidc_error)?;
    let candidates = transaction
        .query_all_raw(Statement::from_sql_and_values(
            backend,
            discovery,
            values.clone(),
        ))
        .await
        .map_err(Error::internal)?;
    let owners = external_logout_owners(&candidates)?;
    lock_external_logout_users(transaction, db, owners.values().copied()).await?;
    let rows = transaction
        .query_all_raw(Statement::from_sql_and_values(backend, lookup, values))
        .await
        .map_err(Error::internal)?;
    // 不允许锁定查询出现新关联或owner变化；失败回滚整个事务（包括token墓碑）。
    for row in &rows {
        let id: String = row.try_get("", "id_hash").map_err(Error::internal)?;
        verified_external_logout_owner(
            &owners,
            &id,
            row.try_get("", "user_id").map_err(Error::internal)?,
        )?;
    }
    Ok(rows)
}
fn external_logout_owners(rows: &[sea_orm::QueryResult]) -> Result<BTreeMap<String, i64>> {
    let mut owners = BTreeMap::new();
    for row in rows {
        let id: String = row.try_get("", "id_hash").map_err(Error::internal)?;
        let user_id: i64 = row.try_get("", "user_id").map_err(Error::internal)?;
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        if let Some(previous) = owners.insert(id, user_id)
            && previous != user_id
        {
            return Err(Error::Unauthorized);
        }
    }
    Ok(owners)
}
fn verified_external_logout_owner(
    owners: &BTreeMap<String, i64>,
    id: &str,
    actual: i64,
) -> Result<i64> {
    if actual <= 0 || owners.get(id).copied() != Some(actual) {
        return Err(Error::Unauthorized);
    }
    Ok(actual)
}
async fn lock_external_logout_users(
    transaction: &DatabaseTransaction,
    db: &Db,
    owners: impl IntoIterator<Item = i64>,
) -> Result<()> {
    let users = db.quoted_table_name("users")?;
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!("SELECT id FROM {users} WHERE id = $1 FOR UPDATE"),
        DbBackend::MySql => format!("SELECT id FROM {users} WHERE id = ? FOR UPDATE"),
        _ => return Err(oidc_error()),
    };
    for user_id in owners.into_iter().collect::<BTreeSet<_>>() {
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
    }
    Ok(())
}
async fn lock_external_logout_scope(
    transaction: &DatabaseTransaction,
    db: &Db,
    provider: &str,
    issuer: &str,
    scope_hash: &str,
    revoke_until: Option<chrono::DateTime<Utc>>,
) -> Result<bool> {
    let table = db.quoted_table_name("auth_external_logout_scopes")?;
    let backend = transaction.get_database_backend();
    let prune = match backend {
        DbBackend::Postgres => format!(
            "DELETE FROM {table} WHERE (provider, issuer, scope_hash) IN (SELECT provider, issuer, scope_hash FROM {table} WHERE expires_at <= CURRENT_TIMESTAMP ORDER BY expires_at, provider, issuer, scope_hash LIMIT 100)"
        ),
        DbBackend::MySql => format!(
            "DELETE FROM {table} WHERE (provider, issuer, scope_hash) IN (SELECT provider, issuer, scope_hash FROM (SELECT provider, issuer, scope_hash FROM {table} WHERE expires_at <= CURRENT_TIMESTAMP(6) ORDER BY expires_at, provider, issuer, scope_hash LIMIT 100) AS expired_scopes)"
        ),
        _ => return Err(oidc_error()),
    };
    transaction
        .execute_raw(Statement::from_string(backend, prune))
        .await
        .map_err(Error::internal)?;
    let insert = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (provider, issuer, scope_hash, expires_at, created_at) VALUES ($1, $2, $3, NULL, CURRENT_TIMESTAMP) ON CONFLICT (provider, issuer, scope_hash) DO NOTHING"
        ),
        DbBackend::MySql => format!(
            "INSERT IGNORE INTO {table} (provider, issuer, scope_hash, expires_at, created_at) VALUES (?, ?, ?, NULL, CURRENT_TIMESTAMP(6))"
        ),
        _ => return Err(oidc_error()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &insert,
            [
                provider.to_owned().into(),
                issuer.to_owned().into(),
                scope_hash.to_owned().into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    let select = match backend {
        DbBackend::Postgres => format!(
            "SELECT expires_at FROM {table} WHERE provider = $1 AND issuer = $2 AND scope_hash = $3 FOR UPDATE"
        ),
        DbBackend::MySql => format!(
            "SELECT expires_at FROM {table} WHERE provider = ? AND issuer = ? AND scope_hash = ? FOR UPDATE"
        ),
        _ => return Err(oidc_error()),
    };
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &select,
            [
                provider.to_owned().into(),
                issuer.to_owned().into(),
                scope_hash.to_owned().into(),
            ],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(oidc_error)?;
    let expires_at: Option<chrono::DateTime<Utc>> =
        row.try_get("", "expires_at").map_err(Error::internal)?;
    if let Some(revoke_until) = revoke_until {
        let update = match backend {
            DbBackend::Postgres => format!(
                "UPDATE {table} SET expires_at = CASE WHEN expires_at IS NULL OR expires_at < $4 THEN $4 ELSE expires_at END WHERE provider = $1 AND issuer = $2 AND scope_hash = $3"
            ),
            DbBackend::MySql => format!(
                "UPDATE {table} SET expires_at = CASE WHEN expires_at IS NULL OR expires_at < ? THEN ? ELSE expires_at END WHERE provider = ? AND issuer = ? AND scope_hash = ?"
            ),
            _ => return Err(oidc_error()),
        };
        let values = if backend == DbBackend::Postgres {
            vec![
                provider.to_owned().into(),
                issuer.to_owned().into(),
                scope_hash.to_owned().into(),
                revoke_until.into(),
            ]
        } else {
            vec![
                revoke_until.into(),
                revoke_until.into(),
                provider.to_owned().into(),
                issuer.to_owned().into(),
                scope_hash.to_owned().into(),
            ]
        };
        transaction
            .execute_raw(Statement::from_sql_and_values(backend, &update, values))
            .await
            .map_err(Error::internal)?;
    }
    Ok(expires_at.is_some_and(|expiry| expiry > Utc::now()))
}

async fn release_external_logout_scope(
    transaction: &DatabaseTransaction,
    db: &Db,
    provider: &str,
    issuer: &str,
    scope_hash: &str,
) -> Result<()> {
    let table = db.quoted_table_name("auth_external_logout_scopes")?;
    let backend = transaction.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!(
            "DELETE FROM {table} WHERE provider = $1 AND issuer = $2 AND scope_hash = $3 AND (expires_at IS NULL OR expires_at <= CURRENT_TIMESTAMP)"
        ),
        DbBackend::MySql => format!(
            "DELETE FROM {table} WHERE provider = ? AND issuer = ? AND scope_hash = ? AND (expires_at IS NULL OR expires_at <= CURRENT_TIMESTAMP(6))"
        ),
        _ => return Err(oidc_error()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [
                provider.to_owned().into(),
                issuer.to_owned().into(),
                scope_hash.to_owned().into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
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

    fn end_session_endpoint(&self) -> Option<url::Url> {
        self.end_session_endpoint.clone()
    }

    fn post_logout_redirect_uri(&self) -> Option<String> {
        Some(self.redirect_url.as_str().to_owned())
    }

    fn client_id(&self) -> Option<&str> {
        Some(self.client_id.as_str())
    }

    fn verify_logout_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedLogoutToken>> {
        Box::pin(std::future::ready(
            self.verify_backchannel_logout_token(token),
        ))
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

    fn logout_token(
        key: &EncodingKey,
        key_id: &str,
        claims: &serde_json::Value,
    ) -> std::result::Result<String, jsonwebtoken::errors::Error> {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(key_id.to_owned());
        header.typ = Some("logout+jwt".to_owned());
        encode(&header, &claims, key)
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

    #[test]
    fn external_identity_session_id_is_validated_and_redacted() -> Result<()> {
        let identity = VerifiedExternalIdentity::new(
            "https://issuer.example.test",
            "subject-1",
            Some("person@example.test".to_owned()),
            true,
        )?
        .with_session_id("private-idp-session")?;
        assert_eq!(identity.session_id(), Some("private-idp-session"));
        let formatted = format!("{identity:?}");
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains("private-idp-session"));
        assert!(identity.clone().with_session_id("bad\nvalue").is_err());
        Ok(())
    }

    #[test]
    fn external_login_handle_can_be_restored_from_secure_storage() -> Result<()> {
        let issued = ExternalLoginHandle(random_secret()?);
        let restored = ExternalLoginHandle::from_exposed(issued.expose())?;
        assert_eq!(restored.expose(), issued.expose());
        assert!(ExternalLoginHandle::from_exposed("not-a-login-handle").is_err());
        let formatted = format!("{restored:?}");
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains(restored.expose()));
        Ok(())
    }

    #[test]
    fn backchannel_logout_token_requires_signature_audience_event_and_no_nonce() -> Result<()> {
        let key = EncodingKey::from_ec_pem(PRIVATE_KEY).map_err(Error::internal)?;
        let jwks = test_keyset(&key, "logout-key")?;
        let allowed = HashSet::from([Algorithm::ES256]);
        let now = jsonwebtoken::get_current_timestamp();
        let event = "http://schemas.openid.net/event/backchannel-logout";
        let claims = json!({
            "iss": "https://issuer.example.test",
            "aud": "openrust-client",
            "exp": now + 600,
            "iat": now,
            "jti": "logout-jti-1",
            "events": { "http://schemas.openid.net/event/backchannel-logout": {} },
            "sid": "provider-session-1"
        });
        let signed = logout_token(&key, "logout-key", &claims).map_err(Error::internal)?;
        let verified = OidcProvider::verify_logout_token(
            &signed,
            "https://issuer.example.test",
            "openrust-client",
            &jwks,
            &allowed,
        )?;
        assert_eq!(verified.session_id(), Some("provider-session-1"));
        assert_eq!(verified.subject(), None);

        let mut with_nonce = claims.clone();
        with_nonce["nonce"] = json!("forbidden");
        for invalid in [
            with_nonce,
            {
                let mut value = claims.clone();
                value["aud"] = json!("another-client");
                value
            },
            {
                let mut value = claims.clone();
                value["events"][event] = json!({"unexpected": true});
                value
            },
        ] {
            let signed = logout_token(&key, "logout-key", &invalid).map_err(Error::internal)?;
            assert!(
                OidcProvider::verify_logout_token(
                    &signed,
                    "https://issuer.example.test",
                    "openrust-client",
                    &jwks,
                    &allowed,
                )
                .is_err()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod logout_lock_order_tests {
    use super::*;
    use sea_orm::TransactionTrait;
    use sea_orm_migration::prelude::*;

    #[test]
    fn locked_association_must_keep_its_prelocked_owner() {
        let owners = BTreeMap::from([("a".to_owned(), 1), ("b".to_owned(), 2)]);
        assert_eq!(
            verified_external_logout_owner(&owners, "a", 1).ok(),
            Some(1)
        );
        assert!(verified_external_logout_owner(&owners, "a", 2).is_err());
        assert!(verified_external_logout_owner(&owners, "new", 1).is_err());
        assert!(verified_external_logout_owner(&owners, "a", 0).is_err());
    }
    struct LockMigrator;
    struct LockSchema;
    impl MigrationName for LockSchema {
        fn name(&self) -> &'static str {
            "m20261105_000001_oidc_lock_graph_fixture"
        }
    }
    #[sea_orm_migration::async_trait::async_trait]
    impl MigrationTrait for LockSchema {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            for sql in [
                "CREATE TABLE users (id BIGINT PRIMARY KEY)",
                "INSERT INTO users VALUES (1),(2)",
                "CREATE TABLE auth_external_sessions (id_hash VARCHAR(64) PRIMARY KEY,user_id BIGINT NOT NULL,refresh_family_id VARCHAR(64) NULL)",
                "INSERT INTO auth_external_sessions VALUES ('a',1,NULL),('b',2,NULL)",
                "CREATE TABLE auth_refresh_tokens (id BIGINT PRIMARY KEY,token_hash VARCHAR(64) NOT NULL UNIQUE,family_id VARCHAR(64),user_id BIGINT NOT NULL,revoked_at TIMESTAMP NULL)",
                "INSERT INTO auth_refresh_tokens VALUES (1,'hash-foreign','foreign',2,NULL),(2,'hash-mixed-1','mixed',1,NULL),(3,'hash-mixed-2','mixed',2,NULL)",
            ] {
                manager.get_connection().execute_unprepared(sql).await?;
            }
            Ok(())
        }
        async fn down(&self, _manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            Ok(())
        }
    }
    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for LockMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(LockSchema)]
        }
    }

    struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);
    impl<T> Drop for AbortOnDrop<T> {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "真实双事务与NOWAIT探针保持同一可审查锁序回归"
    )]
    #[ignore = "真实PG/MySQL行锁barrier；缺少OPENRUST_TEST_DATABASE_URL即错误，不以SQLite代替"]
    async fn real_logout_waits_for_all_sorted_users_before_locking_any_association() -> Result<()> {
        let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
        let database = crate::testing::TestDatabase::create::<LockMigrator>(&url).await?;
        // 两个持锁事务加独立NOWAIT探针需要三条连接；不改通用fixture的两连接预算。
        let mut options = crate::database::sea_orm::ConnectOptions::new(database.url());
        options.max_connections(4).min_connections(0);
        let connection = crate::database::sea_orm::Database::connect(options)
            .await
            .map_err(Error::internal)?;
        let db = Db::default();
        db.initialize(connection.clone())?;
        let result: Result<()> = async {
            let connection = db.connection()?; let backend = connection.get_database_backend();
            if !matches!(backend, DbBackend::MySql | DbBackend::Postgres) { return Err(oidc_error()); }
            let holder = connection.begin().await.map_err(Error::internal)?;
            lock_external_logout_users(&holder, &db, [2]).await?;
            let association = db.quoted_table_name("auth_external_sessions")?;
            let lookup = format!("SELECT id_hash,user_id,refresh_family_id FROM {association} ORDER BY id_hash FOR UPDATE");
            let contender_db = db.clone(); let contender_connection = connection.clone();
            let mut task = AbortOnDrop(tokio::spawn(async move {
                let transaction = contender_connection.begin().await.map_err(Error::internal)?;
                let rows = lock_external_logout_associations(&transaction, &contender_db, backend, &lookup, Vec::new()).await?;
                let ids = rows.iter().map(|row| row.try_get::<i64>("", "user_id").map_err(Error::internal)).collect::<Result<Vec<_>>>()?;
                for row in &rows {
                    let owner = row.try_get::<i64>("", "user_id").map_err(Error::internal)?;
                    if let Some(family) = row.try_get::<Option<String>>("", "refresh_family_id").map_err(Error::internal)? {
                        crate::auth::refresh::revoke_family_for_locked_owner_in_transaction(&transaction, &contender_db, backend, &family, owner).await?;
                    }
                }
                transaction.commit().await.map_err(Error::internal)?;
                Ok::<_, Error>(ids)
            }));
            // 数据库NOWAIT探针确认contender已经锁users1并等待users2，不靠sleep猜交错。
            let users = db.quoted_table_name("users")?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                if tokio::time::Instant::now() >= deadline { task.0.abort(); return Err(Error::custom("test_lock_barrier_timeout", "未观察到users升序等待")); }
                let probe = connection.begin().await.map_err(Error::internal)?;
                let attempted = probe.query_one_raw(Statement::from_string(backend, format!("SELECT id FROM {users} WHERE id=1 FOR UPDATE NOWAIT"))).await;
                probe.rollback().await.map_err(Error::internal)?;
                match attempted {
                    Err(error) => {
                        let message = error.to_string();
                        if message.contains("could not obtain lock") || message.contains("3572") || message.contains("NOWAIT") { break; }
                        task.0.abort(); return Err(Error::internal(error));
                    }
                    Ok(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
            let available = tokio::time::timeout(Duration::from_secs(2), holder.query_one_raw(Statement::from_string(backend, format!("SELECT user_id FROM {association} WHERE id_hash='b' FOR UPDATE")))).await;
            if available.is_err() { task.0.abort(); return Err(Error::custom("test_inverse_lock_order", "注销先占有关联导致MFA账户锁等待环")); }
            available.map_err(Error::internal)?.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
            let refresh = db.quoted_table_name("auth_refresh_tokens")?;
            holder.execute_unprepared(&format!("INSERT INTO {refresh} VALUES (4,'hash-late','late-family',2,NULL)")).await.map_err(Error::internal)?;
            holder.execute_unprepared(&format!("UPDATE {association} SET refresh_family_id='late-family' WHERE id_hash='b'")).await.map_err(Error::internal)?;
            holder.commit().await.map_err(Error::internal)?;
            let ids = tokio::time::timeout(Duration::from_secs(5), &mut task.0).await.map_err(Error::internal)?.map_err(Error::internal)??;
            assert_eq!(ids, vec![1,2]);
            let late = connection.query_one_raw(Statement::from_string(backend, format!("SELECT CASE WHEN revoked_at IS NULL THEN 0 ELSE 1 END AS revoked FROM {refresh} WHERE family_id='late-family'"))).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
            assert_eq!(late.try_get::<i32>("", "revoked").map_err(Error::internal)?, 1);
            let transaction = connection.begin().await.map_err(Error::internal)?;
            lock_external_logout_users(&transaction, &db, [1]).await?;
            assert!(crate::auth::refresh::revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "foreign", 1).await.is_err());
            assert!(crate::auth::refresh::revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "mixed", 1).await.is_err());
            crate::auth::refresh::revoke_family_for_locked_owner_in_transaction(&transaction, &db, backend, "missing", 1).await?;
            transaction.rollback().await.map_err(Error::internal)?;
            Ok(())
        }.await;
        drop(db);
        let closed = connection.close().await.map_err(Error::internal);
        let cleanup = database.close().await;
        result?;
        closed?;
        cleanup
    }
}
