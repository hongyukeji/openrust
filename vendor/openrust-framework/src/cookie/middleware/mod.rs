//! Inbound cookie decoding and outbound cookie processing.

mod add_queued_cookies_to_response;
mod encrypt_cookies;

pub use add_queued_cookies_to_response::AddQueuedCookiesToResponse;
pub use encrypt_cookies::EncryptCookies;

pub(crate) use add_queued_cookies_to_response::request_scope;
pub(super) use add_queued_cookies_to_response::with_queue;
