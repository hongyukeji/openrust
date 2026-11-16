//! Shared framework extension points.

mod arr;
mod benchmark;
pub(crate) mod calendar;
mod collection;
mod data;
mod date;

mod deferred_provider;
mod enumerable;
pub mod facades;
mod fluent;
mod helpers;
mod html_string;
mod lazy_collection;
mod lottery;
pub(crate) mod markdown;
mod message_bag;
mod number;
pub(crate) mod publishing;
mod service_provider;
mod sleep;
pub(crate) use publishing::PublishableResource;
mod stringable;
mod strings;
mod timebox;
mod uri;
mod view_error_bag;

pub use arr::{Arr, ArrValueError};
pub use benchmark::Benchmark;
pub use collection::{ArrayKey, Collection, CollectionError, CollectionNumber, CollectionResult};
pub use data::{DataPathError, data_fill, data_forget, data_get, data_has, data_set};
pub use date::{
    Date, DateError, DateInterval, DateTimezone, MonthOverflow, days, hours, microseconds,
    milliseconds, minutes, months, now, seconds, today, weeks, years,
};
pub use deferred_provider::DeferredProvider;
pub use enumerable::Enumerable;
pub use fluent::{Fluent, FluentError};
pub use helpers::{Retry, blank, filled};
pub use html_string::HtmlString;
pub use lazy_collection::{LazyCollection, LazyIterator, LazySource};
pub use lottery::{InvalidLottery, Lottery};
pub use markdown::{MarkdownHtml, MarkdownOptions};
pub use message_bag::MessageBag;
pub use number::{Number, NumberError, NumberValue};
pub use service_provider::{ProviderFactory, ProviderResult, ServiceProvider};
pub use sleep::{Sleep, SleepFake};
pub use stringable::{ScanError, ScanValue, Stringable};
pub use strings::{
    EmptyPasswordAlphabet, InflectionLanguage, OrderedUuidError, PasswordOptions, Str,
    WordWrapError,
};
pub use timebox::Timebox;
pub use uri::{InvalidUri, Uri, UriQueryString};
pub use view_error_bag::ViewErrorBag;
