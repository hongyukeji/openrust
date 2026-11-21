#![forbid(unsafe_code)]
#![allow(non_snake_case, reason = "Preserves Laravel Pennant API names")]

mod commands;
pub mod concerns;
pub mod contracts;
pub mod drivers;
pub mod events;
mod feature;
mod feature_manager;
pub mod middleware;
mod migration;
mod pennant_service_provider;
mod scope;
pub use feature::Feature;
#[path = "../config/pennant.rs"]
mod configuration;
#[path = "../database/migrations/2026_11_21_000000_create_features_table.rs"]
mod create_features_table;
pub use feature_manager::FeatureManager;
pub use migration::CreateFeaturesTable;
pub use pennant_service_provider::PennantServiceProvider;
mod pending_scoped_feature_interaction;
pub use contracts::{
    Driver, FeatureDefinition, FeatureFuture, FeatureImplementation, FeatureScopeable,
    FeatureScopes, FeatureValue, FeatureValues, Resolver,
};
pub use pending_scoped_feature_interaction::PendingScopedFeatureInteraction;
pub use scope::{FeatureScopeSerializeable, Scope};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    InvalidScope,
    Unsupported(&'static str),
    Auth(openrust::auth::AuthError),
    StateUnavailable,
    Database(openrust::database::DatabaseError),
    InvalidConfiguration,
    InvalidValue,
    MultipleScopes,
    Binding(openrust::contracts::container::BindingResolutionError),
    Event(openrust::events::EventError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unsupported(_) => "The Pennant driver does not support this operation",
            Self::Auth(_) => "Pennant could not resolve the authenticated scope",
            Self::InvalidScope => "Unable to serialize the feature scope",
            Self::StateUnavailable => "Pennant state is unavailable",
            Self::Database(_) => "Pennant database operation failed",
            Self::InvalidConfiguration => "Invalid Pennant store configuration",
            Self::InvalidValue => "Invalid stored feature value",
            Self::MultipleScopes => "It is not possible to retrieve the values for multiple scopes",
            Self::Binding(_) => "Pennant services are unavailable",
            Self::Event(_) => "Pennant event listener failed",
        })
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Auth(source) => Some(source),
            Self::Database(source) => Some(source),
            Self::Binding(source) => Some(source),
            Self::Event(source) => Some(source),
            _ => None,
        }
    }
}
impl From<openrust::database::DatabaseError> for Error {
    fn from(source: openrust::database::DatabaseError) -> Self {
        Self::Database(source)
    }
}
pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(PennantServiceProvider)]
}
