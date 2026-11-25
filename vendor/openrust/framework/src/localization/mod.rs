//! Translation lookup, file loading, and plural selection.

pub(crate) mod bundled;

mod choice_case;
mod facade;
mod file_loader;
mod locale;
mod localization_service_provider;
mod localizer;
mod lookup;
mod message_selector;
mod replacement;

use std::{collections::BTreeMap, future::Future, pin::Pin};

pub use facade::{__, Lang, trans_choice};
pub use file_loader::FileLoader;
pub use locale::canonical_locale;
pub use localization_service_provider::LocalizationServiceProvider;
pub use localizer::Localizer;
pub use lookup::MissingTranslation;
pub use message_selector::{MessageSelector, PluralCount, Selector};
pub use replacement::Replacement;

/// Identifies one exact group to load from an asynchronous translation source.
#[derive(Clone, Copy, Debug)]
pub struct TranslationResource<'a> {
    pub locale: &'a str,
    pub group: &'a str,
    pub namespace: Option<&'a str>,
}

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
    InvalidTag,
    MissingStringable(&'static str),
    StringableFormat(std::fmt::Error),
    CatalogChanged,
    Source(Box<dyn std::error::Error + Send + Sync>),
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCount => formatter.write_str("Invalid translation plural count."),
            Self::InvalidLocale => {
                formatter.write_str("无效的 BCP 47 语言标签；旧下划线形式请按升级说明迁移。")
            }
            Self::InvalidKey => formatter.write_str("Invalid translation key."),
            Self::UnexpectedType => {
                formatter.write_str("Translation value has an unexpected type.")
            }
            Self::InvalidTag => formatter.write_str("Invalid translation tag name."),
            Self::MissingStringable(name) => {
                write!(formatter, "No translation formatter for {name}.")
            }
            Self::StringableFormat(_) => {
                formatter.write_str("Could not format translation object.")
            }
            Self::CatalogChanged => {
                formatter.write_str("Translation catalog changed during loading.")
            }
            Self::Source(source) => write!(formatter, "翻译或语言设置失败：{source}"),
            Self::Unavailable(_) => formatter.write_str("Translation service is unavailable."),
        }
    }
}

impl std::error::Error for TranslationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(source) => Some(source.as_ref()),
            Self::StringableFormat(source) => Some(source),
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

pub(crate) use localizer::LocaleSnapshot;

impl From<crate::contracts::container::BindingResolutionError> for TranslationError {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::Unavailable(source)
    }
}
