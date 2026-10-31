//! 严格配置的 JWT access token 签名与校验。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{ConfigError, JwtConfig};
use crate::core::{BoxFuture, Error, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};

const ACCESS_TOKEN_TYPE: &str = "openrust-at+jwt";
const MIN_KEY_BYTES: usize = 32;
const MAX_IAT_FUTURE_SKEW_SECS: u64 = 30;

/// OpenRust access token 的共享签名与验证器。
#[derive(Clone)]
pub struct JwtTokens {
    inner: Arc<JwtTokenConfig>,
}

impl std::fmt::Debug for JwtTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JwtTokens")
            .field("issuer", &self.inner.issuer)
            .field("audience", &self.inner.audience)
            .field("active_key_id", &self.inner.active_key_id)
            .field("key_ids", &self.inner.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl JwtTokens {
    /// 校验配置并构建可复用的签名/验证密钥环。
    pub fn new(config: JwtConfig) -> Result<Self> {
        validate_config(&config)?;
        let mut keys = BTreeMap::new();
        for (id, encoded) in config.keys {
            let secret = STANDARD
                .decode(encoded.trim())
                .map_err(|_| invalid_config("JWT key values must be valid standard Base64"))?;
            if secret.len() < MIN_KEY_BYTES {
                return Err(invalid_config(
                    "each decoded JWT key must contain at least 32 bytes",
                ));
            }
            keys.insert(
                id,
                JwtKeyMaterial {
                    encoding: EncodingKey::from_secret(&secret),
                    decoding: DecodingKey::from_secret(&secret),
                },
            );
        }
        Ok(Self {
            inner: Arc::new(JwtTokenConfig {
                issuer: config.issuer,
                audience: config.audience,
                active_key_id: config.active_key_id,
                keys,
                access_ttl_secs: config.access_ttl_secs,
                max_valid_ttl_secs: config.max_valid_ttl_secs,
            }),
        })
    }

    /// 为正数框架账户 ID 签发短期 access token。
    pub fn issue_access_token(&self, user_id: i64) -> Result<String> {
        if user_id <= 0 {
            return Err(invalid_config("JWT subject must be a positive account ID"));
        }
        let issued_at = unix_seconds()?;
        let expires_at = issued_at
            .checked_add(self.inner.access_ttl_secs)
            .ok_or_else(|| invalid_config("JWT expiration is out of range"))?;
        let mut jti_bytes = [0_u8; 32];
        getrandom::fill(&mut jti_bytes).map_err(Error::internal)?;
        let claims = AccessClaims {
            iss: self.inner.issuer.clone(),
            aud: self.inner.audience.clone(),
            sub: user_id.to_string(),
            jti: encode_hex(&jti_bytes),
            iat: issued_at,
            nbf: issued_at.saturating_sub(MAX_IAT_FUTURE_SKEW_SECS),
            exp: expires_at,
        };
        let key = self
            .inner
            .keys
            .get(&self.inner.active_key_id)
            .ok_or_else(|| Error::internal(MissingActiveKey))?;
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(self.inner.active_key_id.clone());
        header.typ = Some(ACCESS_TOKEN_TYPE.to_owned());
        encode(&header, &claims, &key.encoding).map_err(|_| Error::internal(TokenSigningError))
    }

    /// 验证 access token 并返回通过身份验证的框架账户 ID。
    pub fn verify_access_token(&self, token: &str) -> Result<i64> {
        self.verify_access_token_with_expiry(token)
            .map(|(user_id, _)| user_id)
    }

    /// 验证 access token，并返回账户 ID 与签名声明中的失效时刻。
    ///
    /// 实时连接应使用这个截止时间，避免自行猜测凭据的剩余有效期。
    pub fn verify_access_token_with_expiry(&self, token: &str) -> Result<(i64, SystemTime)> {
        let header = decode_header(token).map_err(|_| Error::Unauthorized)?;
        if header.alg != Algorithm::HS256 || header.typ.as_deref() != Some(ACCESS_TOKEN_TYPE) {
            return Err(Error::Unauthorized);
        }
        let key_id = header.kid.as_deref().ok_or(Error::Unauthorized)?;
        let key = self.inner.keys.get(key_id).ok_or(Error::Unauthorized)?;
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        validation.validate_nbf = true;
        validation.set_issuer(&[self.inner.issuer.as_str()]);
        validation.set_audience(&[self.inner.audience.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub", "nbf"]);
        let data = decode::<AccessClaims>(token, &key.decoding, &validation)
            .map_err(|_| Error::Unauthorized)?;
        let claims = data.claims;
        let now = unix_seconds().map_err(|_| Error::Unauthorized)?;
        if claims.iss != self.inner.issuer
            || claims.aud != self.inner.audience
            || !valid_jti(&claims.jti)
            || claims.exp <= claims.iat
            || claims.exp - claims.iat > self.inner.max_valid_ttl_secs
            || claims.iat > now.saturating_add(MAX_IAT_FUTURE_SKEW_SECS)
            || claims.nbf > claims.iat
            || claims.iat - claims.nbf > MAX_IAT_FUTURE_SKEW_SECS
        {
            return Err(Error::Unauthorized);
        }
        let user_id = claims.sub.parse::<i64>().map_err(|_| Error::Unauthorized)?;
        if user_id <= 0 || user_id.to_string() != claims.sub {
            return Err(Error::Unauthorized);
        }
        let expires_at = UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(claims.exp))
            .ok_or(Error::Unauthorized)?;
        Ok((user_id, expires_at))
    }
}

/// 从当前配置注册 JWT 签发与验证服务。
#[derive(Debug, Default, Clone, Copy)]
pub struct JwtServiceProvider;

impl crate::core::ServiceProvider for JwtServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            app.config_section(
                "auth.jwt",
                serde_json::json!({}),
                S::object([
                    ("issuer", S::String),
                    ("audience", S::String),
                    ("active_key_id", S::String),
                    ("keys", S::map(S::String.sensitive())),
                    ("access_ttl_secs", S::Integer),
                    ("max_valid_ttl_secs", S::Integer),
                ]),
                |value| {
                    let config: JwtConfig = serde_json::from_value(value.clone())
                        .map_err(|_| invalid_config("auth.jwt configuration is invalid"))?;
                    JwtTokens::new(config).map(|_| ())
                },
            )?;
            app.singleton::<JwtTokens, _>(|resolver| {
                Box::pin(async move {
                    let config = resolver.section::<JwtConfig>("auth.jwt")?;
                    Ok(Arc::new(JwtTokens::new(config)?))
                })
            })
            .await?;
            app.require::<JwtTokens>();
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app.resolve::<JwtTokens>().await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::{BootContext, ProviderBuilder, ServiceProvider};
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    struct Rotate;
    impl ServiceProvider for Rotate {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("auth.jwt", json!({"active_key_id":"new"})) })
        }
    }

    #[tokio::test]
    async fn provider_uses_overlay_key_and_keeps_old_token_verification() -> Result<()> {
        let old = STANDARD.encode([1u8; 32]);
        let new = STANDARD.encode([2u8; 32]);
        let prior = JwtTokens::new(JwtConfig::new(
            "issuer",
            "audience",
            "old",
            BTreeMap::from([("old".into(), old.clone())]),
        ))?;
        let token = prior.issue_access_token(42)?;
        let sources = Figment::from(Serialized::defaults(
            json!({"auth":{"jwt":{"issuer":"issuer","audience":"audience","active_key_id":"old","keys":{"old":old,"new":new}}}}),
        ));
        let mut builder = ProviderBuilder::new();
        builder.provider(Rotate).provider(JwtServiceProvider);
        let runtime = builder.start(sources).await?;
        let tokens = runtime.container().resolve::<JwtTokens>().await?;
        assert_eq!(tokens.verify_access_token(&token)?, 42);
        let header = jsonwebtoken::decode_header(&tokens.issue_access_token(42)?)
            .map_err(Error::internal)?;
        assert_eq!(header.kid.as_deref(), Some("new"));
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    struct InvalidLate(Arc<std::sync::atomic::AtomicBool>);
    impl ServiceProvider for InvalidLate {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            let closed = Arc::clone(&self.0);
            Box::pin(async move {
                app.resources().on_shutdown(move || {
                    Box::pin(async move {
                        closed.store(true, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                app.overlay("auth.jwt", json!({"active_key_id":"missing"}))
            })
        }
    }

    #[tokio::test]
    async fn late_invalid_overlay_fails_and_cleans_resources() {
        let secret = STANDARD.encode([1u8; 32]);
        let source = Figment::from(Serialized::defaults(
            json!({"auth":{"jwt":{"issuer":"i","audience":"a","active_key_id":"key","keys":{"key":secret}}}}),
        ));
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(JwtServiceProvider)
            .provider(InvalidLate(Arc::clone(&closed)));
        let result = builder.start(source).await;
        assert!(result.is_err());
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn provider_rejects_bad_or_missing_secret_without_disclosure() {
        for source in [
            json!({}),
            json!({"auth":{"jwt":{"issuer":"i","audience":"a","active_key_id":"key","keys":{"key":"secret-sentinel"}}}}),
        ] {
            let mut builder = ProviderBuilder::new();
            builder.provider(JwtServiceProvider);
            let result = builder
                .start(Figment::from(Serialized::defaults(source)))
                .await;
            assert!(result.is_err());
            if let Err(error) = result {
                assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
            }
        }
    }
}

struct JwtTokenConfig {
    issuer: String,
    audience: String,
    active_key_id: String,
    keys: BTreeMap<String, JwtKeyMaterial>,
    access_ttl_secs: u64,
    max_valid_ttl_secs: u64,
}

struct JwtKeyMaterial {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AccessClaims {
    iss: String,
    aud: String,
    sub: String,
    jti: String,
    iat: u64,
    nbf: u64,
    exp: u64,
}

fn validate_config(config: &JwtConfig) -> Result<()> {
    if config.issuer.trim().is_empty() || config.audience.trim().is_empty() {
        return Err(invalid_config("JWT issuer and audience must be non-empty"));
    }
    if !valid_key_id(&config.active_key_id) {
        return Err(invalid_config("active JWT key ID is invalid"));
    }
    if config.keys.is_empty() || !config.keys.contains_key(&config.active_key_id) {
        return Err(invalid_config(
            "active JWT key ID must exist in the key ring",
        ));
    }
    if config.keys.keys().any(|id| !valid_key_id(id)) {
        return Err(invalid_config(
            "JWT key IDs must be 1-64 ASCII token characters",
        ));
    }
    if config.access_ttl_secs == 0 || config.access_ttl_secs > 3_600 {
        return Err(invalid_config(
            "JWT access_ttl_secs must be between 1 and 3600",
        ));
    }
    if config.max_valid_ttl_secs < config.access_ttl_secs || config.max_valid_ttl_secs > 3_600 {
        return Err(invalid_config(
            "JWT max_valid_ttl_secs must be at least access_ttl_secs and at most 3600",
        ));
    }
    Ok(())
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_jti(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(Error::internal)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn invalid_config(message: &str) -> Error {
    Error::Config(ConfigError::Invalid(message.to_owned()))
}

#[derive(Debug)]
struct MissingActiveKey;

impl std::fmt::Display for MissingActiveKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("configured active JWT key is unavailable")
    }
}

impl std::error::Error for MissingActiveKey {}

#[derive(Debug)]
struct TokenSigningError;

impl std::fmt::Display for TokenSigningError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JWT access-token signing failed")
    }
}

impl std::error::Error for TokenSigningError {}

#[cfg(test)]
#[allow(clippy::expect_used)] // Fixed test keys/config are valid fixtures; failures should identify the setup.
mod tests {
    use std::collections::BTreeMap;

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};

    use super::{AccessClaims, JwtConfig, JwtTokens, unix_seconds};

    const OLD_SECRET: [u8; 32] = [0x31; 32];
    const NEW_SECRET: [u8; 32] = [0x42; 32];

    fn config(active: &str, keys: &[(&str, &[u8])]) -> JwtConfig {
        JwtConfig::new(
            "https://issuer.example",
            "openrust-tests",
            active,
            keys.iter()
                .map(|(id, key)| ((*id).to_owned(), STANDARD.encode(key)))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn signed_token(claims: &AccessClaims, typ: &str, algorithm: Algorithm) -> String {
        let mut header = Header::new(algorithm);
        header.kid = Some("old".to_owned());
        header.typ = Some(typ.to_owned());
        encode(&header, claims, &EncodingKey::from_secret(&OLD_SECRET)).unwrap_or_default()
    }

    fn valid_claims() -> AccessClaims {
        let now = unix_seconds().unwrap_or_default();
        AccessClaims {
            iss: "https://issuer.example".to_owned(),
            aud: "openrust-tests".to_owned(),
            sub: "73".to_owned(),
            jti: "ab".repeat(32),
            iat: now,
            nbf: now,
            exp: now + 60,
        }
    }

    #[test]
    fn access_token_signs_and_verifies_with_redacted_secret_debug() {
        let config = config("old", &[("old", &OLD_SECRET)]);
        let encoded_key = STANDARD.encode(OLD_SECRET);
        assert!(!format!("{config:?}").contains(&encoded_key));
        let tokens = JwtTokens::new(config).expect("test configuration is valid");
        let token = tokens
            .issue_access_token(73)
            .expect("access token is signed");
        assert_eq!(tokens.verify_access_token(&token).unwrap_or_default(), 73);
        let (user_id, expires_at) = tokens
            .verify_access_token_with_expiry(&token)
            .expect("signed token exposes its verified expiry");
        assert_eq!(user_id, 73);
        assert!(expires_at > std::time::SystemTime::now());
        assert!(expires_at <= std::time::SystemTime::now() + std::time::Duration::from_secs(3_600));
        assert!(tokens.issue_access_token(0).is_err());
    }

    #[test]
    fn key_rotation_accepts_old_tokens_and_signs_with_the_active_key() {
        let old_tokens = JwtTokens::new(config("old", &[("old", &OLD_SECRET)]))
            .expect("old configuration is valid");
        let old_token = old_tokens
            .issue_access_token(73)
            .expect("old key signs token");
        let rotated = JwtTokens::new(config("new", &[("old", &OLD_SECRET), ("new", &NEW_SECRET)]))
            .expect("overlapping key ring is valid");
        assert_eq!(
            rotated.verify_access_token(&old_token).unwrap_or_default(),
            73
        );
        let new_token = rotated.issue_access_token(91).expect("new key signs token");
        assert_eq!(
            rotated.verify_access_token(&new_token).unwrap_or_default(),
            91
        );
    }

    #[test]
    fn rejects_invalid_key_length_and_active_key_configuration() {
        let too_short = [0x11; 31];
        assert!(JwtTokens::new(config("old", &[("old", &too_short)])).is_err());
        assert!(JwtTokens::new(config("missing", &[("old", &OLD_SECRET)])).is_err());
        assert!(JwtTokens::new(config("old", &[("old", &OLD_SECRET)]).with_ttl(0, 3600)).is_err());
    }

    #[test]
    fn rejects_wrong_token_type_algorithm_time_audience_and_subject() {
        let tokens =
            JwtTokens::new(config("old", &[("old", &OLD_SECRET)])).expect("configuration is valid");
        let mut claims = valid_claims();
        assert_unauthorized(tokens.verify_access_token(&signed_token(
            &claims,
            "JWT",
            Algorithm::HS256,
        )));
        assert_unauthorized(tokens.verify_access_token(&signed_token(
            &claims,
            "openrust-at+jwt",
            Algorithm::HS384,
        )));
        claims.exp = claims.iat;
        assert_unauthorized(tokens.verify_access_token(&signed_token(
            &claims,
            "openrust-at+jwt",
            Algorithm::HS256,
        )));
        claims = valid_claims();
        claims.exp = claims.iat + 3_601;
        assert_unauthorized(tokens.verify_access_token(&signed_token(
            &claims,
            "openrust-at+jwt",
            Algorithm::HS256,
        )));
        claims = valid_claims();
        claims.aud = "other-api".to_owned();
        assert_unauthorized(tokens.verify_access_token(&signed_token(
            &claims,
            "openrust-at+jwt",
            Algorithm::HS256,
        )));
        claims = valid_claims();
        claims.sub = "0".to_owned();
        assert_unauthorized(tokens.verify_access_token(&signed_token(
            &claims,
            "openrust-at+jwt",
            Algorithm::HS256,
        )));
    }

    #[allow(clippy::needless_pass_by_value)]
    fn assert_unauthorized(result: crate::core::Result<i64>) {
        assert!(matches!(result, Err(crate::core::Error::Unauthorized)));
    }
}
