//! Middleware for HTTP protocol behavior.

mod handle_cors;
mod set_cache_headers;

pub use handle_cors::HandleCors;
pub(crate) use handle_cors::SkipCallback as CorsSkipCallback;
pub use set_cache_headers::SetCacheHeaders;
pub(super) use set_cache_headers::set_public_cache_control;
