//! League resource verification over the selected application's real token store.
use crate::{OAuthError, Passport, PublicKey};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::{
    contracts::container::Container as _,
    http::Request,
    support::{Date, facades::Facade},
};
use std::sync::Arc;

/// Verified request attributes, corresponding to Passport's lightweight AccessToken.
#[derive(Clone, serde::Serialize)]
pub struct AccessToken {
    pub oauth_access_token_id: String,
    pub oauth_client_id: String,
    pub oauth_user_id: Option<String>,
    pub oauth_scopes: Option<Vec<String>>,
    #[serde(skip)]
    pub(crate) transient: bool,
}
impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccessToken([REDACTED])")
    }
}
impl AccessToken {
    pub fn can(&self, scope: &str) -> openrust::database::DatabaseResult<bool> {
        if self.transient {
            return Ok(true);
        }
        let Some(scopes) = &self.oauth_scopes else {
            return Ok(false);
        };
        if scopes.is_empty() {
            return Ok(false);
        }
        let scopes = crate::PhpArray::from(scopes.clone());
        Ok(scopes.contains_loose("*") || crate::client::scope_exists_in(scope, &scopes)?)
    }
    pub fn cant(&self, scope: &str) -> openrust::database::DatabaseResult<bool> {
        self.can(scope).map(|v| !v)
    }
    pub fn transient(&self) -> bool {
        self.transient
    }
    pub async fn revoke(&self) -> openrust::database::DatabaseResult<bool> {
        if self.transient {
            return Ok(false);
        }
        let passport = openrust::container::Container::getInstance()
            .make::<Passport>()
            .map_err(openrust::database::DatabaseError::Unavailable)?;
        passport
            .token_models
            .revoke_access(&self.oauth_access_token_id)
            .await
    }
}
/// Validates RS256, optional time claims, and the authoritative jti revocation record.
pub struct ResourceServer {
    passport: Arc<Passport>,
    key: Arc<PublicKey>,
}
impl Facade for ResourceServer {
    type Service = Self;
    fn getFacadeAccessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<Self>()
    }
}
#[allow(non_snake_case)]
impl ResourceServer {
    pub(crate) fn new(passport: Arc<Passport>, key: Arc<PublicKey>) -> Self {
        Self { passport, key }
    }
    pub async fn validateAuthenticatedRequest(
        &self,
        request: &mut Request,
    ) -> Result<AccessToken, OAuthError> {
        if let Some(token) = request
            .parts()
            .extensions
            .get::<crate::testing::TestingToken>()
        {
            return Ok(token.0.clone());
        }
        let authorization = request.headers().get("authorization").map(|h| h.as_bytes());
        let token = bearer(authorization)?;
        let now = Date::now(None)
            .map_err(internal)?
            .asDateTime()
            .timestamp_micros() as f64
            / 1_000_000.;
        let claims = self.verify(token, now)?;
        if self
            .passport
            .token_models
            .is_access_revoked(&claims.oauth_access_token_id)
            .await
            .map_err(internal)?
        {
            return Err(OAuthError::access_denied(Some(
                "Access token has been revoked",
            )));
        }
        // A typed extension replaces four dynamic PSR request attributes.
        request.parts_mut().extensions.insert(claims.clone());
        Ok(claims)
    }
    fn verify(&self, token: &str, now: f64) -> Result<AccessToken, OAuthError> {
        use rsa::{
            pkcs1::DecodeRsaPublicKey as _, pkcs8::DecodePublicKey as _,
            traits::PublicKeyParts as _,
        };
        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid());
        };
        let header: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        if header.get("alg").and_then(|v| v.as_str()) != Some("RS256")
            || header.contains_key("enc")
            || header.contains_key("")
        {
            return Err(invalid());
        }
        let key = rsa::PublicKey::from_public_key_pem(self.key.getKeyContents())
            .or_else(|_| rsa::PublicKey::from_pkcs1_pem(self.key.getKeyContents()))
            .map_err(internal)?;
        let signed = token.rsplit_once('.').ok_or_else(invalid)?.0;
        aws_lc_rs::signature::RsaPublicKeyComponents {
            n: key
                .n()
                .to_be_bytes()
                .iter()
                .skip_while(|byte| **byte == 0)
                .copied()
                .collect::<Vec<_>>(),
            e: key
                .e()
                .to_be_bytes()
                .iter()
                .skip_while(|byte| **byte == 0)
                .copied()
                .collect::<Vec<_>>(),
        }
        .verify(
            &aws_lc_rs::signature::RSA_PKCS1_2048_8192_SHA256,
            signed.as_bytes(),
            &URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        let claims: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        if claims.contains_key("") {
            return Err(invalid());
        }
        for name in ["iat", "nbf", "exp"] {
            if let Some(value) = claims.get(name) {
                let time = match value {
                    serde_json::Value::Number(n) => n.as_f64(),
                    serde_json::Value::String(s) => s
                        .trim_matches(|c: char| c.is_ascii_whitespace())
                        .parse::<f64>()
                        .ok(),
                    _ => None,
                }
                .filter(|n| n.is_finite())
                .ok_or_else(invalid)?;
                // Match PHP NumericDate's six-place U.u parsing, including its signed-second range.
                let normalized = format!("{time:.6}");
                let (seconds, micros) = normalized.split_once('.').ok_or_else(invalid)?;
                let seconds = seconds.parse::<i64>().map_err(|_| invalid())?;
                let micros = micros.parse::<u32>().map_err(|_| invalid())?;
                let time = seconds as f64 + f64::from(micros) / 1_000_000.;
                if if name == "exp" {
                    now >= time
                } else {
                    now < time
                } {
                    return Err(invalid());
                }
            }
        }
        let client = match claims.get("aud") {
            Some(serde_json::Value::String(s)) => Some(s.as_str()),
            Some(serde_json::Value::Array(a)) => a.first().and_then(|v| v.as_str()),
            _ => None,
        }
        .filter(|s| !s.is_empty())
        .ok_or_else(invalid)?;
        let id = claims
            .get("jti")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(invalid)?;
        let user = match claims.get("sub") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            _ => return Err(invalid()),
        };
        let scopes = match claims.get("scopes") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => {
                Some(serde_json::from_value::<Vec<String>>(value.clone()).map_err(|_| invalid())?)
            }
        };
        Ok(AccessToken {
            oauth_access_token_id: id.into(),
            oauth_client_id: client.into(),
            oauth_user_id: user,
            oauth_scopes: scopes,
            transient: false,
        })
    }
}
fn internal(_: impl std::fmt::Display) -> OAuthError {
    OAuthError::server_error("OAuth resource validation failed")
}
fn invalid() -> OAuthError {
    OAuthError::access_denied(Some("Access token could not be verified"))
}
// Reused from the pinned old ResourceServer: PCRE scheme removal then PHP trim.
fn bearer(authorization: Option<&[u8]>) -> Result<&str, OAuthError> {
    let value = authorization
        .ok_or_else(|| OAuthError::access_denied(Some("Missing \"Authorization\" header")))?;
    let leading = value
        .iter()
        .position(|byte| !pcre_space(*byte))
        .unwrap_or(value.len());
    let candidate = &value[leading..];
    let raw = if candidate
        .get(..6)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"Bearer"))
        && candidate.get(6).is_some_and(|byte| pcre_space(*byte))
    {
        &candidate[7..]
    } else {
        value
    };
    let token = std::str::from_utf8(raw)
        .map_err(|_| invalid())?
        .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\0']);
    if token.is_empty() {
        return Err(OAuthError::access_denied(Some("Missing \"Bearer\" token")));
    }
    Ok(token)
}
const fn pcre_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}
