//! Shared framework extension points.

pub mod facades;
mod message_bag;
mod service_provider;
mod view_error_bag;

pub use message_bag::MessageBag;
pub use service_provider::{ProviderResult, ServiceProvider};
pub use view_error_bag::ViewErrorBag;
