//! Application assembly and lifecycle.

mod application;
pub mod bootstrap;
mod bootstrap_error;
pub mod configuration;
pub mod console;
mod entry;
mod environment_detector;
pub mod events;
pub mod exceptions;
pub mod http;
mod inspiring;
mod provider_repository;
pub mod providers;
pub mod support;

pub use application::Application;
pub use bootstrap_error::BootstrapError;
pub use entry::runApplication;
pub use inspiring::Inspiring;
