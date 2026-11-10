//! Session state shared within an HTTP request.

mod array_session_handler;
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
pub use cache_based_session_handler::CacheBasedSessionHandler;
pub use cookie_session_handler::CookieSessionHandler;
pub use database_session_handler::DatabaseSessionHandler;
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
pub struct SessionError {
    _private: (),
}

/// A typed attribute stored in a session.
pub struct Value {
    _private: (),
}
