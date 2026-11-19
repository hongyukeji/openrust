//! Fortify authentication building blocks. HTTP integration is developed separately.
#![forbid(unsafe_code)]

pub mod contracts;
mod error;
mod features;
mod two_factor_authentication_provider;

pub use error::{FortifyError, FortifyResult};
pub use features::Features;
pub use two_factor_authentication_provider::TwoFactorAuthenticationProvider;
