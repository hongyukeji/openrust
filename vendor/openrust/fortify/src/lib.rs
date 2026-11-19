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
