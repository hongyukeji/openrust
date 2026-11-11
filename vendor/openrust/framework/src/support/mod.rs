//! Shared framework extension points.

mod arr;
mod collection;
mod data;
mod deferred_provider;
mod enumerable;
pub mod facades;
mod fluent;
mod lazy_collection;
mod lottery;
mod message_bag;
mod service_provider;
mod stringable;
mod strings;
mod uri;
mod view_error_bag;

pub use arr::Arr;
pub use collection::{ArrayKey, Collection, CollectionError, CollectionNumber, CollectionResult};
pub use data::{DataPathError, data_fill, data_forget, data_get, data_has, data_set};
pub use deferred_provider::DeferredProvider;
pub use enumerable::Enumerable;
pub use fluent::{Fluent, FluentError};
pub use lazy_collection::{LazyCollection, LazyIterator, LazySource};
pub use lottery::{InvalidLottery, Lottery};
pub use message_bag::MessageBag;
pub use service_provider::{ProviderFactory, ProviderResult, ServiceProvider};
pub use stringable::Stringable;
pub use strings::Str;
pub use uri::{InvalidUri, Uri};
pub use view_error_bag::ViewErrorBag;
