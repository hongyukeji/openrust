//! Shared framework extension points.

mod deferred_provider;
pub mod facades;
mod message_bag;
mod service_provider;
mod view_error_bag;

pub use deferred_provider::DeferredProvider;
pub use message_bag::MessageBag;
pub use service_provider::{ProviderFactory, ProviderResult, ServiceProvider};
pub use view_error_bag::ViewErrorBag;
