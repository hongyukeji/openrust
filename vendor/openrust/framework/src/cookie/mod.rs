//! Cookie creation, encryption middleware, and response queues.

mod cookie_definition;
mod cookie_jar;
mod cookie_service_provider;
mod cookie_value_prefix;
pub(crate) mod facade;
pub mod middleware;

pub use cookie_definition::{Cookie, CookieOptions, SameSite};
pub use cookie_jar::CookieJar;
pub use cookie_service_provider::CookieServiceProvider;
pub use cookie_value_prefix::CookieValuePrefix;

/// A cookie creation, encoding, or protection failure.
#[derive(Debug)]
pub struct CookieError;

impl std::fmt::Display for CookieError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid cookie or unavailable cookie scope")
    }
}

impl std::error::Error for CookieError {}

impl From<CookieError> for crate::http::HttpError {
    fn from(source: CookieError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ResponseCookies(pub(crate) Vec<(::http::HeaderValue, Cookie)>);

pub(crate) struct CookieEncryption {
    pub(crate) encrypter: std::sync::Arc<dyn crate::contracts::encryption::Encrypter>,
    pub(crate) exceptions: std::sync::Arc<std::collections::HashSet<String>>,
}
