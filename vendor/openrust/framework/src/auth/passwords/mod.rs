//! Password reset brokers and token repositories.

mod cache_token_repository;
mod database_token_repository;
mod facade;
mod password_broker;
mod password_broker_manager;
mod token_policy;
mod token_repository;

pub use cache_token_repository::CacheTokenRepository;
pub use database_token_repository::DatabaseTokenRepository;
pub use facade::Password;
pub use password_broker::PasswordBroker;
pub use password_broker_manager::PasswordBrokerManager;
pub use token_repository::{TokenClaim, TokenRepository};

/// A broker outcome that can be translated by the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordResetStatus {
    ResetLinkSent,
    PasswordReset,
    InvalidUser,
    InvalidToken,
    ResetThrottled,
}

impl PasswordResetStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResetLinkSent => "passwords.sent",
            Self::PasswordReset => "passwords.reset",
            Self::InvalidUser => "passwords.user",
            Self::InvalidToken => "passwords.token",
            Self::ResetThrottled => "passwords.throttled",
        }
    }
}
