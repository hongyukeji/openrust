//! HS256 first-party cookie, protected by the application's ordinary Cookie middleware.
use crate::{AccessToken, OAuthError, Passport};
use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::{
    auth::AuthIdentifier,
    contracts::container::Container as _,
    cookie::{Cookie, CookieOptions, CookieValuePrefix, SameSite},
    encryption::Encrypter,
    http::Request,
    support::{Date, facades::Config},
};
use serde_json::Value;
use std::sync::atomic::Ordering;
use subtle::ConstantTimeEq as _;
pub struct ApiTokenCookieFactory;
impl ApiTokenCookieFactory {
    pub fn make(user: impl Into<AuthIdentifier>, csrf: &str) -> Result<Cookie, OAuthError> {
        let passport = passport()?;
        let encrypter = openrust::container::Container::get_instance()
            .make::<Encrypter>()
            .map_err(internal)?;
        let session = Config::get("session")
            .map_err(internal)?
            .unwrap_or_default();
        let lifetime = session
            .get("lifetime")
            .and_then(Value::as_i64)
            .unwrap_or(120);
        let exp = now()?
            .checked_add(lifetime.checked_mul(60).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        let jwt = encode(
            &serde_json::json!({"sub":user.into(),"csrf":csrf,"exp":exp}),
            &passport.tokenEncryptionKey(&encrypter),
        )?;
        let options = CookieOptions {
            path: Some(
                session
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("/")
                    .into(),
            ),
            domain: session
                .get("domain")
                .and_then(Value::as_str)
                .map(str::to_owned),
            secure: session.get("secure").and_then(Value::as_bool),
            http_only: true,
            same_site: match session.get("same_site").and_then(Value::as_str) {
                Some("strict") => Some(SameSite::Strict),
                Some("none") => Some(SameSite::None),
                Some("lax") => Some(SameSite::Lax),
                _ => None,
            },
            partitioned: session
                .get("partitioned")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..Default::default()
        };
        openrust::support::facades::Cookie::make(&passport.cookie(None), &jwt, lifetime, &options)
            .map_err(internal)
    }
}
pub(crate) fn authenticate(
    request: &Request,
    passport: &Passport,
    encrypter: &Encrypter,
) -> Result<Option<AuthIdentifier>, OAuthError> {
    let Some(Value::String(value)) = request
        .cookie(Some(&passport.cookie(None)))
        .map_err(internal)?
    else {
        return Ok(None);
    };
    let plain = if passport.decryptsCookies.load(Ordering::Relaxed) {
        let decoded =
            match encrypter.decrypt(&value, passport.unserializesCookies.load(Ordering::Relaxed)) {
                Ok(v) => zeroize::Zeroizing::new(v),
                Err(_) => return Ok(None),
            };
        String::from_utf8(CookieValuePrefix::remove(&decoded).to_vec()).map_err(|_| invalid())?
    } else {
        value
    };
    let token = match decode(&plain, &passport.tokenEncryptionKey(encrypter)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if !passport.ignoreCsrfToken.load(Ordering::Relaxed) {
        let mut supplied = request
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty() && *v != "0")
            .map(str::to_owned);
        if supplied.is_none() {
            supplied = request
                .headers()
                .get("x-xsrf-token")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| encrypter.decrypt(v, false).ok())
                .and_then(|v| String::from_utf8(CookieValuePrefix::remove(&v).to_vec()).ok());
        }
        if !matches!((token.get("csrf").and_then(Value::as_str),supplied.as_deref()),(Some(a),Some(b)) if bool::from(a.as_bytes().ct_eq(b.as_bytes())))
        {
            return Ok(None);
        }
    }
    match token.get("sub") {
        Some(Value::String(v)) => Ok(Some(AuthIdentifier::from(v.clone()))),
        Some(Value::Number(v)) if v.as_i64().is_some() => {
            Ok(Some(AuthIdentifier::Integer(v.as_i64().unwrap())))
        }
        _ => Ok(None),
    }
}
pub(crate) fn transient(user: AuthIdentifier) -> AccessToken {
    AccessToken {
        oauth_access_token_id: String::new(),
        oauth_client_id: String::new(),
        oauth_user_id: Some(user.to_string()),
        oauth_scopes: None,
        transient: true,
    }
}
fn encode(claims: &Value, key: &[u8]) -> Result<String, OAuthError> {
    let signing = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(internal)?)
    );
    let signature = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), signing.as_bytes());
    Ok(format!(
        "{signing}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}
fn decode(jwt: &str, key: &[u8]) -> Result<Value, OAuthError> {
    let mut parts = jwt.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid());
    };
    let header: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if header.get("alg").and_then(Value::as_str) != Some("HS256") {
        return Err(invalid());
    }
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, key),
        jwt.rsplit_once('.').ok_or_else(invalid)?.0.as_bytes(),
        &URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    let token: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    let now = now()?;
    for time in ["exp", "expiry"] {
        if let Some(exp) = token.get(time)
            && exp.as_i64().is_none_or(|v| now >= v)
        {
            return Err(invalid());
        }
    }
    for time in ["iat", "nbf"] {
        if let Some(exp) = token.get(time)
            && exp.as_i64().is_none_or(|v| now < v)
        {
            return Err(invalid());
        }
    }
    Ok(token)
}
fn passport() -> Result<std::sync::Arc<Passport>, OAuthError> {
    openrust::container::Container::get_instance()
        .make::<Passport>()
        .map_err(internal)
}
fn now() -> Result<i64, OAuthError> {
    Ok(Date::now(None)
        .map_err(internal)?
        .asDateTime()
        .timestamp()
        .as_nanosecond()
        .div_euclid(1_000_000_000) as i64)
}
fn invalid() -> OAuthError {
    OAuthError::access_denied(Some("Invalid API token cookie"))
}
fn internal(_: impl std::fmt::Display) -> OAuthError {
    OAuthError::server_error("API token cookie unavailable")
}
