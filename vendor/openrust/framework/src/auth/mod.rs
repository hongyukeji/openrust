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
pub use database_user_provider::{DatabaseUser, DatabaseUserProvider};
pub use eloquent_user_provider::EloquentUserProvider;
pub use request_guard::RequestGuard;
pub use session_guard::SessionGuard;
pub use token_guard::TokenGuard;

/// An asynchronous authentication or authorization operation.
pub type AuthFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub use identity::{AuthError, AuthIdentifier, Credentials};

/// An authentication operation's result.
pub type AuthResult<T> = Result<T, AuthError>;

mod facade;
mod identity;
mod providers;
pub use facade::Auth;

pub(crate) fn reset_request(request: &mut crate::http::Request) {
    request
        .parts_mut()
        .extensions
        .remove::<session_guard::AuthContext>();
}
