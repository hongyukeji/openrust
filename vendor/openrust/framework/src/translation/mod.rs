//! Translation lookup, file loading, and plural selection.

mod choice_case;
mod facade;
mod file_loader;
mod message_selector;
mod translation_service_provider;
mod translator;

use std::{collections::BTreeMap, future::Future, pin::Pin};

pub use facade::{__, Lang, trans_choice};
pub use file_loader::FileLoader;
pub use message_selector::{MessageSelector, PluralCount};
pub use translation_service_provider::TranslationServiceProvider;
pub use translator::Translator;

/// A loaded translation group.
pub type Lines = BTreeMap<String, Line>;

/// A translation entry or a nested group of entries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(untagged)]
pub enum Line {
    Text(String),
    Group(Lines),
    List(Vec<Line>),
}

/// An asynchronous translation source operation.
pub type TranslationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of loading or reading a translation group.
pub type TranslationResult<T> = Result<T, TranslationError>;

/// An invalid locale or group, a source failure, or a mismatched entry type.
#[derive(Debug)]
#[non_exhaustive]
pub enum TranslationError {
    InvalidCount,
    InvalidLocale,
    InvalidKey,
    UnexpectedType,
    Source(Box<dyn std::error::Error + Send + Sync>),
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCount => formatter.write_str("Invalid translation plural count."),
            Self::InvalidLocale => formatter.write_str("Invalid translation locale."),
            Self::InvalidKey => formatter.write_str("Invalid translation key."),
            Self::UnexpectedType => {
                formatter.write_str("Translation value has an unexpected type.")
            }
            Self::Source(_) => formatter.write_str("Could not load translation catalog."),
            Self::Unavailable(_) => formatter.write_str("Translation service is unavailable."),
        }
    }
}

impl std::error::Error for TranslationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(source) => Some(source.as_ref()),
            Self::Unavailable(source) => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn language_path(root: &std::path::Path) -> std::path::PathBuf {
    let legacy = root.join("resources/lang");
    if legacy.is_dir() {
        legacy
    } else {
        root.join("lang")
    }
}
