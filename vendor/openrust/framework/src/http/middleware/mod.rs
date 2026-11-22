//! Middleware for HTTP protocol behavior.

mod handle_cors;
mod set_cache_headers;
mod set_locale;
mod validate_path_encoding;
mod validate_post_size;

pub use handle_cors::HandleCors;
pub(crate) use handle_cors::SkipCallback as CorsSkipCallback;
pub use set_cache_headers::SetCacheHeaders;
pub(super) use set_cache_headers::set_public_cache_control;
pub use set_locale::SetLocale;
pub use validate_path_encoding::ValidatePathEncoding;
pub use validate_post_size::ValidatePostSize;

mod prefers_json_responses;
pub use prefers_json_responses::PrefersJsonResponses;
