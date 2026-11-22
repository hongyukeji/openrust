//! Environment snapshot and application configuration repository.

pub(crate) mod cache;
mod environment;
mod error;
mod repository;

pub use environment::Environment;
pub use error::ConfigError;
pub(crate) use repository::LocaleOverrides;
pub use repository::Repository;
pub use serde_json::Value;
