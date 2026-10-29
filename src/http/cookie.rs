//! 请求 Cookie、响应属性与请求隔离队列。
mod crypto;
pub(crate) mod middleware;
mod parse;
mod provider;
mod queue;
mod response;
mod value;

pub use ::cookie::SameSite;
pub(crate) use provider::CookieRuntime;
pub use provider::CookieServiceProvider;
pub use queue::CookieJar;
pub use response::{CookieResponse, CookieResponseExt};
pub use value::{Cookie, CookieFactory, CookieOptions};

pub(crate) use parse::{OriginalCookies, ParsedCookies, parse_headers};

pub(crate) fn cookie_limits_error() -> crate::Error {
    crate::Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "input_cookie_limits_exceeded",
        "Cookie 超过资源限制",
    )
}
