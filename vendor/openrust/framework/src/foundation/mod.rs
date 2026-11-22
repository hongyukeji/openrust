//! Application assembly and lifecycle.

mod application;
pub mod auth;
pub mod bootstrap;
mod bootstrap_error;
pub mod configuration;
pub mod console;
mod dev_commands;
mod entry;
mod environment_detector;
pub mod events;
pub mod exceptions;
pub mod http;
mod inspiring;
mod locale;
pub mod maintenance;
mod package_manifest;
pub(crate) mod paths;
mod precognition;
mod provider_repository;
pub mod providers;
mod runner;
mod runner_pattern;
pub mod support;

pub use application::Application;
pub use bootstrap_error::BootstrapError;
pub use dev_commands::{DevCommand, DevCommandMode, DevCommands, DevProcess};
pub use entry::run_application;
pub use inspiring::Inspiring;
pub use package_manifest::{DiscoveredPackage, PackageManifest};
pub use paths::Paths;
pub use precognition::Precognition;
pub use runner::{Runner, RunnerContext, RunningService, RuntimeResult, wait_for_shutdown};
