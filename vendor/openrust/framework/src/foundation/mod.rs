//! Application assembly and lifecycle.

mod application;
pub mod auth;
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
pub mod maintenance;
mod provider_repository;
pub mod providers;
mod runner;
mod runner_pattern;
pub mod support;

pub use application::Application;
pub use bootstrap_error::BootstrapError;
pub use entry::runApplication;
pub use inspiring::Inspiring;
pub use runner::{Runner, RunnerContext, RunningService, RuntimeResult, wait_for_shutdown};
