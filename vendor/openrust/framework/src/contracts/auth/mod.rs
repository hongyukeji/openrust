//! Authentication and authorization extension contracts.

pub mod access;
mod authenticatable;
mod factory;
mod guard;
mod stateful_guard;
mod user_provider;

pub use authenticatable::Authenticatable;
pub use factory::Factory;
pub use guard::Guard;
pub use stateful_guard::StatefulGuard;
pub use user_provider::UserProvider;

mod can_reset_password;
mod must_verify_email;
mod password_broker;
pub use can_reset_password::CanResetPassword;
pub use must_verify_email::MustVerifyEmail;
pub use password_broker::{PasswordBroker, ResetCallback, ResetLinkCallback};
