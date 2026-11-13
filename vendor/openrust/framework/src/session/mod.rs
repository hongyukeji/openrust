//! Session state shared within an HTTP request.

mod array_session_handler;
mod blocking;
mod cache_based_session_handler;
mod cookie_session_handler;
mod database_session_handler;
mod encrypted_store;
mod file_session_handler;
pub mod middleware;
mod null_session_handler;
mod session_manager;
mod session_service_provider;
mod store;

use std::{future::Future, pin::Pin};

pub use array_session_handler::ArraySessionHandler;
pub(crate) use blocking::{RouteBlock, SessionLock};
pub use cache_based_session_handler::CacheBasedSessionHandler;
pub use cookie_session_handler::CookieSessionHandler;
pub use database_session_handler::{DatabaseSessionHandler, DatabaseSessionMetadata};
pub use encrypted_store::EncryptedStore;
pub use file_session_handler::FileSessionHandler;
pub use null_session_handler::NullSessionHandler;
pub use session_manager::SessionManager;
pub use session_service_provider::SessionServiceProvider;
pub use store::Store;

/// An asynchronous session or storage operation.
pub type SessionFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a session operation.
pub type SessionResult<T> = Result<T, SessionError>;

/// A session configuration, encoding, or storage failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum SessionError {
    Configuration,
    Encoding,
    PayloadTooLarge,
    Storage,
    Clock,
    Random,
    UnprotectedCookie,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Configuration => "invalid session configuration",
            Self::Encoding => "invalid session payload",
            Self::PayloadTooLarge => "session payload exceeds the configured limit",
            Self::Storage => "session storage is unavailable",
            Self::Clock => "session clock is unavailable",
            Self::Random => "session randomness is unavailable",
            Self::UnprotectedCookie => "session cookie requires encryption",
        })
    }
}

impl std::error::Error for SessionError {}

/// A typed attribute stored in a session.
pub type Value = serde_json::Value;

impl From<SessionError> for crate::http::HttpError {
    fn from(source: SessionError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}
