//! Translation lookup, file loading, and plural selection.

mod file_loader;
mod message_selector;
mod translation_service_provider;
mod translator;

use std::{collections::BTreeMap, future::Future, pin::Pin};

pub use file_loader::FileLoader;
pub use message_selector::MessageSelector;
pub use translation_service_provider::TranslationServiceProvider;
pub use translator::Translator;

/// A loaded translation group.
pub type Lines = BTreeMap<String, Line>;

/// A translation entry or a nested group of entries.
pub enum Line {
    Text(String),
    Group(Lines),
}

/// An asynchronous translation source operation.
pub type TranslationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of loading or reading a translation group.
pub type TranslationResult<T> = Result<T, TranslationError>;

/// An invalid locale or group, a source failure, or a mismatched entry type.
#[derive(Debug)]
pub struct TranslationError {
    _private: (),
}
