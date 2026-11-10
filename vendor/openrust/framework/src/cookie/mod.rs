//! Cookie creation, encryption middleware, and response queues.

mod cookie_definition;
mod cookie_jar;
mod cookie_service_provider;
mod cookie_value_prefix;
pub mod middleware;

pub use cookie_definition::{Cookie, CookieOptions};
pub use cookie_jar::CookieJar;
pub use cookie_service_provider::CookieServiceProvider;
pub use cookie_value_prefix::CookieValuePrefix;

/// A cookie creation, encoding, or protection failure.
#[derive(Debug)]
pub struct CookieError {
    _private: (),
}
