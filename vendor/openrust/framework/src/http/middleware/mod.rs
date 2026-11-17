//! Middleware for HTTP protocol behavior.

mod handle_cors;
mod set_cache_headers;

pub use handle_cors::HandleCors;
pub use set_cache_headers::SetCacheHeaders;
