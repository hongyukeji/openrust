//! Authentication drivers, user providers, and authorization.

pub mod access;
mod auth_manager;
mod auth_service_provider;
mod basic;
pub(crate) mod console;
mod database_user_provider;
mod eloquent_user_provider;
pub mod events;
pub mod listeners;
pub mod middleware;
mod model_class;
pub mod notifications;
pub mod passwords;
mod request_guard;
pub(crate) mod session_fingerprint;
mod session_guard;
mod token_guard;

use std::{future::Future, pin::Pin};

/// Configuration class reference with a compiled Eloquent authentication factory.
/// `model!(crate::app::models::User)` corresponds to PHP's `User::class`.
pub use crate::__auth_model as model;
pub use auth_manager::AuthManager;
pub use auth_service_provider::AuthServiceProvider;
pub use database_user_provider::{DatabaseUser, DatabaseUserProvider};
pub use eloquent_user_provider::EloquentUserProvider;
#[doc(hidden)]
pub use inventory as __inventory;
#[doc(hidden)]
pub use model_class::ModelClass as __ModelClass;
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

pub(crate) fn logged_user(
    attributes: &crate::http::RequestAttributes,
    default: &str,
) -> Option<std::sync::Arc<dyn crate::contracts::auth::Authenticatable>> {
    session_guard::AuthContext::logged_user(attributes, default)
}

pub(crate) fn reset_request(request: &mut crate::http::Request) {
    request
        .attributes()
        .remove(std::any::type_name::<session_guard::AuthContext>());
    #[cfg(feature = "sanctum")]
    request
        .parts_mut()
        .extensions
        .remove::<crate::sanctum::RequestTokens>();
    request
        .parts_mut()
        .extensions
        .remove::<session_guard::AuthContext>();
}
