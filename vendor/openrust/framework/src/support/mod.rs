//! Shared framework extension points.

mod collection;
mod deferred_provider;
mod enumerable;
pub mod facades;
mod fluent;
mod lazy_collection;
mod lottery;
mod message_bag;
mod service_provider;
mod stringable;
mod uri;
mod view_error_bag;

pub use collection::{ArrayKey, Collection, CollectionError, CollectionNumber, CollectionResult};
pub use deferred_provider::DeferredProvider;
pub use enumerable::Enumerable;
pub use fluent::{Fluent, FluentError};
pub use lazy_collection::{LazyCollection, LazyIterator, LazySource};
pub use lottery::{InvalidLottery, Lottery};
pub use message_bag::MessageBag;
pub use service_provider::{ProviderFactory, ProviderResult, ServiceProvider};
pub use stringable::Stringable;
pub use uri::{InvalidUri, Uri};
pub use view_error_bag::ViewErrorBag;
