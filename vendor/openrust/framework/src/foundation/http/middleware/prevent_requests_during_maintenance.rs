use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ::http::{
    HeaderMap, HeaderName, HeaderValue,
    header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, RETRY_AFTER},
};
use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use subtle::ConstantTimeEq as _;

use crate::{
    contracts::{foundation::MaintenanceMode, http::Middleware},
    cookie::CookieOptions,
    foundation::maintenance::MaintenancePayload,
    http::{Body, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode, abort},
    support::facades::{Cookie, Redirect},
};

const BYPASS_COOKIE: &str = "laravel_maintenance";
const BYPASS_MINUTES: i64 = 12 * 60;

/// Enforces application maintenance state before route middleware runs.
pub struct PreventRequestsDuringMaintenance {
    mode: Arc<dyn MaintenanceMode>,
    except: Vec<String>,
}

impl PreventRequestsDuringMaintenance {
    pub fn new(mode: Arc<dyn MaintenanceMode>, except: Vec<String>) -> Self {
        Self { mode, except }
    }
}

impl Middleware for PreventRequestsDuringMaintenance {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if self
                .except
                .iter()
                .any(|path| request.matches_url_or_path(path))
            {
                return Ok(next.run(request).await);
            }
            let data = match self.mode.data().await {
                Ok(Some(data)) => data,
                Ok(None) => return Ok(next.run(request).await),
                Err(error) => {
                    if matches!(self.mode.active().await, Ok(false)) {
                        return Ok(next.run(request).await);
                    }
                    return Err(internal(error));
                }
            };
            let path = request.uri().path().trim_matches('/');
            let path = if path.is_empty() { "/" } else { path };
            if let Some(secret) = data.get("secret").and_then(Value::as_str) {
                if bool::from(secret.as_bytes().ct_eq(path.as_bytes())) {
                    return bypass_response(request, secret);
                }
                if valid_bypass(request, secret) {
                    return Ok(next.run(request).await);
                }
            }

            let status = maintenance_status(&data);
            let headers = maintenance_headers(&data)?;
            if !request.expectsJson() {
                if let Some(target) = data.get("redirect").and_then(Value::as_str) {
                    let target_path = target.trim_matches('/');
                    let target_path = if target_path.is_empty() {
                        "/"
                    } else {
                        target_path
                    };
                    if path != target_path {
                        return Redirect::to(target, Default::default()).map_err(internal);
                    }
                }
                if let Some(template) = data.get("template").and_then(Value::as_str) {
                    return Ok(Response::new(status, Body::from(template.to_owned()))
                        .header(
                            CONTENT_TYPE,
                            HeaderValue::from_static("text/html; charset=UTF-8"),
                        )
                        .withHeaders(headers));
                }
            }

            Err(abort(status, "Service Unavailable", headers))
        })
    }
}

fn maintenance_status(data: &MaintenancePayload) -> StatusCode {
    data.get("status")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .and_then(|value| StatusCode::from_u16(value).ok())
        .unwrap_or(StatusCode::SERVICE_UNAVAILABLE)
}

pub(in crate::foundation::http) fn prerender(
    request: &Request,
    data: &MaintenancePayload,
    except: &[String],
) -> Option<Response> {
    let template = data.get("template")?.as_str()?;
    if request.expectsJson()
        || except
            .iter()
            .map(String::as_str)
            .chain(
                data.get("except")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
            .any(|path| request.matches_url_or_path(path))
    {
        return None;
    }
    if let Some(secret) = data.get("secret").and_then(Value::as_str)
        && (bool::from(
            secret
                .as_bytes()
                .ct_eq(request.uri().path().trim_start_matches('/').as_bytes()),
        ) || valid_bypass(request, secret))
    {
        return None;
    }
    if let Some(target) = data.get("redirect").and_then(Value::as_str)
        && target != request.uri().path()
    {
        if !target.starts_with('/') || target.starts_with("//") || target.contains('\\') {
            return None;
        }
        return Some(
            Response::empty(StatusCode::FOUND)
                .header(LOCATION, HeaderValue::from_str(target).ok()?),
        );
    }
    let head = request.method() == Method::HEAD;
    let body = if head {
        Body::empty()
    } else {
        Body::from(template.to_owned())
    };
    let mut response = Response::new(maintenance_status(data), body)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=UTF-8"),
        )
        .withHeaders(maintenance_headers(data).ok()?);
    if head {
        response = response.header(CONTENT_LENGTH, HeaderValue::from(template.len()));
    }
    Some(response)
}

fn maintenance_headers(data: &MaintenancePayload) -> Result<HeaderMap, HttpError> {
    let mut headers = HeaderMap::new();
    for (key, header) in [
        ("retry", RETRY_AFTER),
        ("refresh", HeaderName::from_static("refresh")),
    ] {
        match data.get(key) {
            Some(Value::String(value)) => {
                headers.insert(header, HeaderValue::from_str(value).map_err(internal)?);
            }
            Some(Value::Number(value)) => {
                headers.insert(
                    header,
                    HeaderValue::from_str(&value.to_string()).map_err(internal)?,
                );
            }
            _ => {}
        }
    }
    Ok(headers)
}

#[derive(Serialize, Deserialize)]
struct Bypass {
    expires_at: u64,
    mac: String,
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn signature(secret: &str, expires_at: u64) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mac = hmac::sign(&key, expires_at.to_string().as_bytes());
    let mut output = String::with_capacity(mac.as_ref().len() * 2);
    use std::fmt::Write as _;
    for byte in mac.as_ref() {
        write!(output, "{byte:02x}").expect("writing to a String succeeds");
    }
    output
}

fn valid_bypass(request: &Request, secret: &str) -> bool {
    request
        .cookie(Some(BYPASS_COOKIE))
        .ok()
        .flatten()
        .and_then(|value| STANDARD.decode(value.as_str()?).ok())
        .and_then(|bytes| serde_json::from_slice::<Bypass>(&bytes).ok())
        .is_some_and(|payload| {
            payload.expires_at >= unix_seconds()
                && bool::from(
                    signature(secret, payload.expires_at)
                        .as_bytes()
                        .ct_eq(payload.mac.as_bytes()),
                )
        })
}

fn bypass_response(request: &mut Request, secret: &str) -> Result<Response, HttpError> {
    let expires_at = unix_seconds() + (BYPASS_MINUTES as u64) * 60;
    let payload = Bypass {
        expires_at,
        mac: signature(secret, expires_at),
    };
    let encoded = STANDARD.encode(serde_json::to_vec(&payload).map_err(internal)?);
    let cookie = Cookie::make(
        BYPASS_COOKIE,
        &encoded,
        BYPASS_MINUTES,
        &CookieOptions::default(),
    )
    .map_err(internal)?;
    let response = if request.session().is_ok() {
        Redirect::intended(request, "/", Default::default())
    } else {
        Redirect::to("/", Default::default())
    }
    .map_err(internal)?;
    response
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store, private"))
        .cookie(&cookie, request)
}

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
