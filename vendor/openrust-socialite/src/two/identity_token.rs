use super::abstract_provider::json;
use crate::{Error, Result};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::Jwk};
use openrust::http::client::Factory;
use serde_json::Value;
use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};
/// Use the JWT library for parsing and signatures; preserve the pinned PHP
/// NumericDate rules, optional dates, strict scalar audience and iat fallback.
pub(super) struct Requirements<'a> {
    pub issuer: &'a str,
    pub audience: &'a str,
    pub algorithm: Option<Algorithm>,
    pub first_key: bool,
}
pub(super) async fn verify(
    http: &Factory,
    url: &str,
    token: &str,
    requirements: Requirements<'_>,
) -> Result<Value> {
    let Requirements {
        issuer,
        audience,
        algorithm,
        first_key,
    } = requirements;
    let invalid = || Error::InvalidIdentityToken;
    let response = json(
        http.pending_request()
            .redact_payload()
            .without_redirecting()
            .get(url)
            .await?,
    )?;
    let keys = response
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let header = decode_header(token).map_err(|_| invalid())?;
    if matches!(
        header.alg,
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
    ) {
        return Err(invalid());
    }
    if algorithm.is_some_and(|algorithm| algorithm != header.alg) {
        return Err(invalid());
    }
    let kid = header
        .kid
        .as_deref()
        .filter(|v| first_key || !v.is_empty())
        .ok_or_else(invalid)?;
    let matching = |key: &&Value| key.get("kid").and_then(Value::as_str) == Some(kid);
    let raw = if first_key {
        keys.iter().find(matching)
    } else {
        keys.iter().rev().find(matching)
    }
    .ok_or_else(invalid)?;
    let key: Jwk = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
    let alg = algorithm
        .map(|v| format!("{v:?}"))
        .or_else(|| key.common.key_algorithm.map(|v| v.to_string()))
        .ok_or_else(invalid)?;
    if Algorithm::from_str(&alg).map_err(|_| invalid())? != header.alg {
        return Err(invalid());
    }
    let key = DecodingKey::from_jwk(&key).map_err(|_| invalid())?;
    let mut validation = Validation::new(header.alg);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.set_audience(&[audience]);
    validation.set_issuer(&[issuer]);
    let claims = decode::<Value>(token, &key, &validation)
        .map_err(|_| invalid())?
        .claims;
    if claims.get("iss").and_then(Value::as_str) != Some(issuer)
        || claims.get("aud").and_then(Value::as_str) != Some(audience)
    {
        return Err(invalid());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?
        .as_secs_f64();
    fn numeric(claims: &Value, name: &str) -> Result<Option<f64>> {
        let Some(value) = claims.get(name).filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let number = match value {
            Value::Number(v) => v.as_f64(),
            Value::String(v) => v.trim().parse::<f64>().ok(),
            _ => None,
        }
        .filter(|v| v.is_finite())
        .ok_or(Error::InvalidIdentityToken)?;
        Ok(Some(number))
    }
    let exp = numeric(&claims, "exp")?;
    let nbf = numeric(&claims, "nbf")?;
    let iat = numeric(&claims, "iat")?;
    if exp.is_some_and(|v| now.floor() >= v)
        || nbf.is_some_and(|v| v.floor() > now.floor())
        || (nbf.is_none() && iat.is_some_and(|v| v.floor() > now.floor()))
    {
        return Err(invalid());
    }
    Ok(claims)
}
