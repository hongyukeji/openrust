//! 解密在 Session 前，响应先附加队列再加密。
use super::{CookieJar, CookieLayerPlan, crypto::Protection};
use crate::container::Scope;
#[cfg(feature = "session")]
use crate::routing::invocation::Invocation;
use crate::routing::{invocation::suppress_cookies, methods::ManagedNext};
use axum::{
    extract::{FromRequestParts, Request},
    response::Response,
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

impl CookieLayer {
    #[cfg(feature = "session")]
    pub(crate) fn is_protected(&self) -> bool {
        !matches!(&self.protection, Protection::Plain)
    }

    #[cfg(feature = "session")]
    pub(crate) fn protects_cookie(&self, name: &str) -> bool {
        self.is_protected() && !self.except.iter().any(|except| except == name)
    }

    #[cfg(feature = "session")]
    pub(crate) fn protects_web_cookies(&self, session_cookie_name: &str) -> bool {
        self.protects_cookie("XSRF-TOKEN") && self.protects_cookie(session_cookie_name)
    }

    #[cfg(feature = "session")]
    pub(crate) fn uses_laravel_payload(&self) -> bool {
        match &self.protection {
            Protection::Plain => false,
            #[cfg(feature = "encryption")]
            Protection::Encrypted(_) => false,
            #[cfg(feature = "encryption")]
            Protection::Laravel(_) => true,
        }
    }

    #[cfg(feature = "session")]
    pub(crate) fn decrypt_xsrf_header(&self, value: &str) -> Option<String> {
        #[cfg(feature = "encryption")]
        if let Protection::Laravel(encrypter) = &self.protection {
            return crate::encryption::decrypt_xsrf_header(encrypter.as_ref(), value);
        }
        let mut root = serde_json::json!({"XSRF-TOKEN": value});
        self.protection.decrypt(&mut root, &[]);
        root.get("XSRF-TOKEN")?.as_str().map(str::to_owned)
    }
}

#[cfg(feature = "session")]
#[derive(Clone)]
struct CookiePreparedContext {
    scope: std::sync::Weak<Scope>,
    issuer: Arc<CookieLayerPlan>,
    layer: Arc<CookieLayer>,
    jar: CookieJar,
}

#[cfg(feature = "session")]
pub(crate) fn clear_prepared(extensions: &mut http::Extensions) {
    extensions.remove::<CookiePreparedContext>();
}

#[cfg(feature = "session")]
pub(crate) fn require_prepared(
    request: &Request,
    expected: &Arc<CookieLayerPlan>,
    invocation: &Invocation,
) -> crate::Result<(Arc<CookieLayer>, CookieJar)> {
    let proof = request.extensions().get::<CookiePreparedContext>();
    let jar = request.extensions().get::<CookieJar>();
    if let Some((proof, jar)) = proof.zip(jar)
        && invocation.owns_scope(request)
        && proof
            .scope
            .upgrade()
            .zip(request.extensions().get::<Arc<Scope>>())
            .is_some_and(|(prepared, current)| Arc::ptr_eq(&prepared, current))
        && Arc::ptr_eq(&proof.issuer, expected)
        && proof.jar.same_queue(jar)
    {
        return Ok((Arc::clone(&proof.layer), jar.clone()));
    }
    Err(crate::Error::custom(
        "cookie_preparation_required",
        "请求缺少本次 Cookie 准备上下文",
    ))
}

pub(crate) async fn handle(
    plan: Arc<CookieLayerPlan>,
    mut request: Request,
    next: ManagedNext,
) -> Response {
    let invocation = next.invocation();
    let Some(scope) = invocation.request_scope(&request) else {
        invocation.fail();
        return super::private_error_response(crate::Error::custom(
            "cookie_scope_missing",
            "Cookie 中间件要求受管理请求作用域",
        ));
    };
    let secure = match crate::http::transport::is_secure(&request) {
        Ok(secure) => secure,
        Err(error) => {
            invocation.fail();
            return super::private_error_response(error);
        }
    };
    let layer = match plan.resolve(&scope).await {
        Ok(layer) => layer,
        Err(error) => {
            invocation.fail();
            return super::private_error_response(error);
        }
    };
    let result = prepare(&layer, &scope, &mut request).await;
    let jar = match result {
        Ok(jar) => jar,
        Err(error) => {
            invocation.fail();
            return super::private_error_response(error);
        }
    };
    #[cfg(feature = "session")]
    request.extensions_mut().insert(CookiePreparedContext {
        scope: Arc::downgrade(&scope),
        issuer: plan,
        layer: Arc::clone(&layer),
        jar: jar.clone(),
    });
    let context = request
        .extensions()
        .get::<crate::support::CalendarContext>()
        .cloned()
        .unwrap_or_default();
    let mut response = next.run(request).await;
    if !invocation.seal() {
        invocation.fail();
        let _ = jar.flush_queued_cookies();
        return suppress_cookies(response);
    }
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
        Err(error) => {
            invocation.fail();
            super::private_error_response(error)
        }
    }
}

async fn prepare(
    layer: &CookieLayer,
    scope: &Scope,
    request: &mut Request,
) -> crate::Result<CookieJar> {
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
    #[cfg(feature = "session")]
    request
        .extensions_mut()
        .insert(layer.protection.authenticity(&layer.except));
    request.extensions_mut().insert(parsed.original.clone());
    request.extensions_mut().insert(parsed);
    let jar = (*scope.resolve::<CookieJar>().await?).clone();
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
