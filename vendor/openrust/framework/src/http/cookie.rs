//! 请求 Cookie、响应属性与请求隔离队列。
mod crypto;
pub(crate) mod middleware;
mod parse;
mod provider;
mod queue;
mod response;
mod value;

pub use ::cookie::SameSite;
#[cfg(feature = "session")]
pub(crate) use crypto::CookieProtection;
#[cfg(feature = "session")]
pub(crate) use middleware::CookieLayer;
pub(crate) use provider::CookieLayerPlan;
pub use provider::CookieServiceProvider;
pub use queue::CookieJar;
#[cfg(feature = "session")]
pub(crate) use response::attach as attach_response_cookies;
pub use response::{CookieResponse, CookieResponseExt};
pub use value::{Cookie, CookieFactory, CookieOptions};

pub(crate) use parse::{OriginalCookies, ParsedCookies, parse_headers};

// Cookie/Session 机制失败不能成为可缓存的敏感响应；不继承旧正文或出站 Cookie。
pub(crate) fn private_error_response(error: crate::Error) -> axum::response::Response {
    use axum::response::IntoResponse;
    let mut response = error.into_response();
    response.headers_mut().insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        http::header::PRAGMA,
        http::HeaderValue::from_static("no-cache"),
    );
    response
}

pub(crate) fn cookie_limits_error() -> crate::Error {
    crate::Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "input_cookie_limits_exceeded",
        "Cookie 超过资源限制",
    )
}
