#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

extern crate self as openrust_pulse;

mod entry;
mod migration;
mod value;
pub use migration::CreatePulseTables;
mod commands;
pub mod events;
pub mod facades;
pub mod http;
pub mod ingests;
mod provider;
mod pulse;
pub mod storage;
mod users;
pub use users::{FieldResolver, LegacyUserResolver, LegacyUsers, ResolvesUsers, User, Users};
pub mod contracts {
    pub use crate::ResolvesUsers;
    pub use crate::ingests::Ingest;
    pub use crate::storage::Storage;
}
pub mod recorders;
pub use provider::PulseServiceProvider;
#[path = "../config/pulse.rs"]
mod configuration;
pub use entry::{Entry, Item};
pub use pulse::Pulse;
pub use value::Value;
pub type Result<T> = std::result::Result<T, Error>;
pub type PulseFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;

#[derive(Debug)]
pub enum Error {
    Redis(openrust::redis::RedisError),
    Event(openrust::events::EventError),
    Io(std::io::Error),
    Auth(openrust::auth::AuthError),
    Cache(openrust::cache::CacheError),
    Container(openrust::contracts::container::BindingResolutionError),
    Database(openrust::database::DatabaseError),
    InvalidAggregate(String),
    InvalidConfiguration(String),
    StateUnavailable,
    InvalidPayload(serde_json::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(e) => write!(f, "Pulse user resolution failed: {e}"),
            Self::Io(e) => write!(f, "Pulse dashboard asset failed: {e}"),
            Self::Event(e) => write!(f, "Pulse event registration failed: {e}"),
            Self::Container(e) => write!(f, "Pulse service resolution failed: {e}"),
            Self::Cache(e) => write!(f, "Pulse cache operation failed: {e}"),
            Self::Redis(e) => write!(f, "Pulse Redis operation failed: {e}"),
            Self::Database(e) => write!(f, "Pulse database operation failed: {e}"),
            Self::InvalidAggregate(a) => write!(
                f,
                "Invalid aggregate type [{a}], allowed types: [count, min, max, sum, avg]."
            ),
            Self::InvalidConfiguration(e) => write!(f, "Invalid Pulse configuration: {e}"),
            Self::StateUnavailable => f.write_str("Pulse state is unavailable"),
            Self::InvalidPayload(e) => write!(f, "Invalid Pulse payload: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Auth(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Event(e) => Some(e),
            Self::Container(e) => Some(e),
            Self::Cache(e) => Some(e),
            Self::Redis(e) => Some(e),
            Self::Database(e) => Some(e),
            Self::InvalidPayload(e) => Some(e),
            _ => None,
        }
    }
}
impl From<openrust::database::DatabaseError> for Error {
    fn from(e: openrust::database::DatabaseError) -> Self {
        Self::Database(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidPayload(e)
    }
}

impl From<openrust::redis::RedisError> for Error {
    fn from(e: openrust::redis::RedisError) -> Self {
        Self::Redis(e)
    }
}

pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(PulseServiceProvider)]
}

impl From<openrust::cache::CacheError> for Error {
    fn from(e: openrust::cache::CacheError) -> Self {
        Self::Cache(e)
    }
}

impl From<openrust::contracts::container::BindingResolutionError> for Error {
    fn from(e: openrust::contracts::container::BindingResolutionError) -> Self {
        Self::Container(e)
    }
}

impl From<openrust::events::EventError> for Error {
    fn from(error: openrust::events::EventError) -> Self {
        Self::Event(error)
    }
}

/// Trusted styles added by the application, never dashboard query input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Css {
    Path(std::path::PathBuf),
    Html(String),
}
