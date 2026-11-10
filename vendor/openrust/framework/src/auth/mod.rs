//! Authentication drivers, user providers, and authorization.

pub mod access;
mod auth_manager;
mod auth_service_provider;
mod database_user_provider;
mod eloquent_user_provider;
pub mod middleware;
mod request_guard;
mod session_guard;
mod token_guard;

use std::{future::Future, pin::Pin};

pub use auth_manager::AuthManager;
pub use auth_service_provider::AuthServiceProvider;
pub use database_user_provider::DatabaseUserProvider;
pub use eloquent_user_provider::EloquentUserProvider;
pub use request_guard::RequestGuard;
pub use session_guard::SessionGuard;
pub use token_guard::TokenGuard;

/// An asynchronous authentication or authorization operation.
pub type AuthFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A resolved authentication identity without numeric coercion.
pub enum AuthIdentifier {
    Integer(i64),
    UnsignedInteger(u64),
    String(String),
}

/// The inputs used to look up a user and verify their credentials.
pub struct Credentials {
    _private: (),
}

/// A failure while resolving or executing authentication.
#[derive(Debug)]
pub struct AuthError {
    _private: (),
}

/// An authentication operation's result.
pub type AuthResult<T> = Result<T, AuthError>;
