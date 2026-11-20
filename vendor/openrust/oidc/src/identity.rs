use crate::{Error, ProviderConfig, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
};
use serde::Deserialize;
use serde_json::Value;
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;
/// Only constructed after signature, claims, nonce and optional UserInfo checks.
#[derive(Clone)]
pub struct VerifiedIdentity {
    pub(crate) issuer: String,
    pub(crate) subject: String,
    pub(crate) email: Option<String>,
    pub(crate) email_verified: bool,
    pub(crate) claims: Value,
    pub(crate) userinfo: Option<Value>,
}
impl VerifiedIdentity {
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }
    pub fn emailVerified(&self) -> bool {
        self.email_verified
    }
    pub fn claims(&self) -> &Value {
        &self.claims
    }
    pub fn userInfo(&self) -> Option<&Value> {
        self.userinfo.as_ref()
    }
}
impl std::fmt::Debug for VerifiedIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedIdentity")
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}
pub struct Tokens {
    pub access_token: Zeroizing<String>,
    pub id_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_in: Option<u64>,
}
/// Opaque logout hint captured from a successfully verified authentication.
/// It deliberately has no Debug, Serialize or Deserialize implementation.
pub struct LogoutHint {
    pub(crate) config: String,
    pub(crate) token: Zeroizing<String>,
}
pub struct Authentication {
    pub identity: VerifiedIdentity,
    pub tokens: Tokens,
    pub(crate) logout_hint: LogoutHint,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
impl Audience {
    fn values(&self) -> Vec<&str> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(a) => a.iter().map(String::as_str).collect(),
        }
    }
}
#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    iat: u64,
    nonce: String,
    #[serde(default)]
    azp: Option<String>,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(default)]
    auth_time: Option<u64>,
    #[serde(default)]
    at_hash: Option<String>,
    #[serde(default)]
    c_hash: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
}
pub(crate) struct Expected<'a> {
    pub config: &'a ProviderConfig,
    pub nonce: &'a str,
    pub code: &'a str,
    pub access_token: &'a str,
    pub algorithms: &'a [Algorithm],
    pub max_age: Option<u64>,
}
pub(crate) fn verify(
    token: &str,
    jwks: &JwkSet,
    expected: Expected<'_>,
) -> Result<VerifiedIdentity> {
    let Expected {
        config,
        nonce,
        code,
        access_token,
        algorithms,
        max_age,
    } = expected;
    let bad = || Error::InvalidToken;
    let header = decode_header(token).map_err(|_| bad())?;
    if header.typ.as_deref().is_some_and(|t| t != "JWT")
        || header.crit.is_some()
        || header.jwk.is_some()
        || header.jku.is_some()
        || header.x5u.is_some()
        || header.x5c.is_some()
        || !algorithms.contains(&header.alg)
        || !config.algorithms.contains(&header.alg)
    {
        return Err(bad());
    }
    let kid = header
        .kid
        .as_deref()
        .filter(|k| !k.is_empty())
        .ok_or_else(bad)?;
    let mut keys = jwks
        .keys
        .iter()
        .filter(|k| k.common.key_id.as_deref() == Some(kid));
    let key = keys.next().ok_or_else(bad)?;
    if keys.next().is_some()
        || key
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|u| *u != PublicKeyUse::Signature)
        || key
            .common
            .key_operations
            .as_ref()
            .is_some_and(|ops| !ops.contains(&KeyOperations::Verify))
        || key
            .common
            .key_algorithm
            .is_some_and(|a| a != KeyAlgorithm::from(header.alg))
    {
        return Err(bad());
    }
    let key = DecodingKey::from_jwk(key).map_err(|_| bad())?;
    let mut validation = Validation::new(header.alg);
    validation.leeway = config.leeway;
    validation.validate_nbf = true;
    validation.set_issuer(&[&config.issuer]);
    validation.set_audience(&[&config.client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "sub", "aud", "iat", "nonce"]);
    let claims_json = decode::<Value>(token, &key, &validation)
        .map_err(|_| bad())?
        .claims;
    let claims: Claims = serde_json::from_value(claims_json.clone()).map_err(|_| bad())?;
    let now = crate::provider::now()?;
    let aud = claims.aud.values();
    if claims.iss != config.issuer
        || claims.sub.trim().is_empty()
        || claims.sub.len() > 255
        || !claims.sub.is_ascii()
        || claims.sub.bytes().any(|b| b.is_ascii_control())
        || !aud.contains(&config.client_id.as_str())
        || aud.iter().any(|a| a.is_empty())
        || claims.azp.as_deref().is_some_and(|a| a != config.client_id)
        || (aud.len() > 1 && claims.azp.as_deref() != Some(config.client_id.as_str()))
        || now >= claims.exp.saturating_add(config.leeway)
        || claims.exp <= claims.iat
        || claims
            .auth_time
            .is_some_and(|t| t > now.saturating_add(config.leeway))
        || claims.iat > now.saturating_add(config.leeway)
        || now
            > claims
                .iat
                .saturating_add(config.max_token_age)
                .saturating_add(config.leeway)
        || claims
            .nbf
            .is_some_and(|t| t > now.saturating_add(config.leeway))
        || nonce.is_empty()
        || !bool::from(claims.nonce.as_bytes().ct_eq(nonce.as_bytes()))
    {
        return Err(bad());
    }
    if let Some(age) = max_age {
        let time = claims.auth_time.ok_or_else(bad)?;
        if time > now.saturating_add(config.leeway)
            || now > time.saturating_add(age).saturating_add(config.leeway)
        {
            return Err(bad());
        }
    }
    for (expected, input) in [(&claims.at_hash, access_token), (&claims.c_hash, code)] {
        if let Some(expected) = expected {
            let algorithm = match header.alg {
                Algorithm::RS256 | Algorithm::PS256 | Algorithm::ES256 => {
                    &aws_lc_rs::digest::SHA256
                }
                Algorithm::RS384 | Algorithm::PS384 | Algorithm::ES384 => {
                    &aws_lc_rs::digest::SHA384
                }
                Algorithm::RS512 | Algorithm::PS512 => &aws_lc_rs::digest::SHA512,
                _ => return Err(bad()),
            };
            let digest = aws_lc_rs::digest::digest(algorithm, input.as_bytes());
            let actual = URL_SAFE_NO_PAD.encode(&digest.as_ref()[..digest.as_ref().len() / 2]);
            if !bool::from(actual.as_bytes().ct_eq(expected.as_bytes())) {
                return Err(bad());
            }
        }
    }
    Ok(VerifiedIdentity {
        issuer: claims.iss,
        subject: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified == Some(true),
        claims: claims_json,
        userinfo: None,
    })
}
