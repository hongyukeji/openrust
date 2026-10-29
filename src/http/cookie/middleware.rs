//! 解密在 Session 前，响应先附加队列再加密。
use super::{CookieJar, crypto::Protection, provider::CookieRuntime};
use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http::{HeaderValue, header::COOKIE, request::Parts};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct CookieLayer {
    pub(crate) protection: Protection,
    pub(crate) except: Vec<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for CookieJar {
    type Rejection = crate::Error;
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> crate::Result<Self> {
        parts.extensions.get::<Self>().cloned().ok_or_else(|| {
            crate::Error::custom(
                "cookie_layer_missing",
                "CookieJar 要求 CookieServiceProvider",
            )
        })
    }
}

pub(crate) fn apply(router: axum::Router, runtime: &CookieRuntime) -> axum::Router {
    match &runtime.0 {
        Some(layer) => router.layer(axum::middleware::from_fn_with_state(layer.clone(), handle)),
        None => router,
    }
}

async fn handle(State(layer): State<CookieLayer>, mut request: Request, next: Next) -> Response {
    let secure = match crate::http::transport::is_secure(&request) {
        Ok(secure) => secure,
        Err(error) => return error.into_response(),
    };
    let result = prepare(&layer, &mut request).await;
    let jar = match result {
        Ok(jar) => jar,
        Err(error) => return error.into_response(),
    };
    let context = request
        .extensions()
        .get::<crate::support::CalendarContext>()
        .cloned()
        .unwrap_or_default();
    let mut response = next.run(request).await;
    let result = (|| {
        let mut cookies = super::response::models_with_context(&response, &context)?;
        for cookie in jar.get_queued_cookies()? {
            super::response::insert_model(&mut cookies, cookie);
        }
        let encrypted = cookies
            .iter()
            .map(|cookie| layer.protection.encrypt(cookie, &layer.except))
            .collect::<crate::Result<Vec<_>>>()?;
        super::response::replace(&mut response, encrypted, secure)
    })();
    match result {
        Ok(()) => response,
        Err(error) => error.into_response(),
    }
}

async fn prepare(layer: &CookieLayer, request: &mut Request) -> crate::Result<CookieJar> {
    let mut parsed = super::parse_headers(request.headers())?;
    layer.protection.decrypt(&mut parsed.value, &layer.except);
    let mut fields = Vec::new();
    flatten(&parsed.value, "", &mut fields);
    let header = fields.join("; ");
    if header.len() > 64 * 1024 {
        return Err(super::cookie_limits_error());
    }
    request.headers_mut().remove(COOKIE);
    if !header.is_empty() {
        request.headers_mut().insert(
            COOKIE,
            HeaderValue::from_str(&header).map_err(crate::Error::internal)?,
        );
    }
    request.extensions_mut().insert(parsed.original.clone());
    request.extensions_mut().insert(parsed);
    let scope = request
        .extensions()
        .get::<Arc<crate::core::container::Scope>>()
        .cloned();
    let jar = match scope {
        Some(scope) => (*scope.resolve::<CookieJar>().await?).clone(),
        None => {
            return Err(crate::Error::custom(
                "cookie_scope_missing",
                "Cookie 中间件要求受管理请求作用域",
            ));
        }
    };
    request.extensions_mut().insert(jar.clone());
    Ok(jar)
}
fn flatten(value: &Value, name: &str, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(format!("{name}={}", encode(value))),
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                flatten(value, &format!("{name}[{index}]"), output);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let child = if name.is_empty() {
                    key.clone()
                } else {
                    format!("{name}[{key}]")
                };
                flatten(value, &child, output);
            }
        }
        _ => {}
    }
}
fn encode(value: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    output
}
