use base64::{Engine as _, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use crate::{
    database::Value,
    http::{HeaderMap, Request, StatusCode, abort},
};

use super::{AuthError, Credentials};

pub(super) fn credentials(
    request: &Request,
    field: &str,
    conditions: &[(String, Value)],
) -> Option<Credentials> {
    let mut headers = request
        .headers()
        .get_all(::http::header::AUTHORIZATION)
        .iter();
    let header = headers.next()?.to_str().ok()?;
    if headers.next().is_some() {
        return None;
    }
    let (scheme, encoded) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = Zeroizing::new(STANDARD.decode(encoded.trim_start_matches(' ')).ok()?);
    let (username, password) = std::str::from_utf8(&decoded).ok()?.split_once(':')?;
    if username.is_empty()
        || username.chars().any(char::is_control)
        || password.chars().any(char::is_control)
    {
        return None;
    }
    let fields = std::iter::once((field.to_owned(), Value::Text(username.to_owned())))
        .chain(conditions.iter().cloned());
    Some(Credentials::new(fields, password))
}

pub(super) fn challenge() -> AuthError {
    let mut headers = HeaderMap::new();
    headers.insert(
        ::http::header::WWW_AUTHENTICATE,
        ::http::HeaderValue::from_static("Basic"),
    );
    abort(StatusCode::UNAUTHORIZED, "Invalid credentials.", headers).into()
}
