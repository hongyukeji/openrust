//! Native Fortify providers, session authentication and application extension contracts.
#![forbid(unsafe_code)]

pub mod contracts;
mod error;
mod features;
mod two_factor_authentication_provider;

pub use error::{FortifyError, FortifyResult};
pub use features::Features;
pub use two_factor_authentication_provider::TwoFactorAuthenticationProvider;

pub mod actions;
#[path = "../config/fortify.rs"]
mod configuration;
mod fortify;
mod fortify_service_provider;
pub mod http;
mod login_rate_limiter;
mod resolver;
pub use fortify::{
    AuthenticateCallback, ConfirmPasswordCallback, Fortify, LoginPipe, LoginPipelineCallback,
};
pub use fortify_service_provider::FortifyServiceProvider;
pub use login_rate_limiter::LoginRateLimiter;

mod recovery_code;
pub use recovery_code::{RecoveryCode, RecoveryCodeGenerator};

pub mod events;
mod two_factor_authenticatable;
pub use two_factor_authenticatable::TwoFactorAuthenticatable;

mod two_factor_model;

mod qr;

mod email_verification_model;

mod password_reset_model;

pub mod console;
mod resources;
pub use resources::FortifyResourcesServiceProvider;

pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(FortifyResourcesServiceProvider)]
}

pub mod migrations;
