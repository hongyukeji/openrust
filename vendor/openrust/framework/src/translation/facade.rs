#![allow(non_snake_case, reason = "Matches Laravel's Lang API")]

use std::{collections::BTreeMap, future::Future};

use crate::{container::ServiceKey, support::facades::Facade};

use super::{Lines, PluralCount, TranslationError, TranslationResult, Translator};

/// Accesses the translator for the current application.
pub struct Lang;

impl Facade for Lang {
    type Service = Translator;

    fn getFacadeAccessor() -> ServiceKey {
        "translator".into()
    }
}

impl Lang {
    pub fn get(
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .string(key, replacements, locale)
    }

    pub fn choice(
        key: &str,
        count: impl Into<PluralCount>,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .choose(key, count, replacements, locale)
    }

    pub fn array(
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .array(key, replacements, locale)
    }

    pub fn getLocale() -> TranslationResult<String> {
        Ok(Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .locale())
    }

    pub async fn withLocale<T>(
        locale: &str,
        operation: impl Future<Output = T>,
    ) -> TranslationResult<T> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .withLocale(locale, operation)
            .await
    }
}

/// Returns a translation from the application's selected locale.
pub fn __(key: &str, replacements: &BTreeMap<String, String>) -> TranslationResult<String> {
    Lang::get(key, replacements, None)
}

/// Selects and interpolates a plural translation.
pub fn trans_choice(
    key: &str,
    count: impl Into<PluralCount>,
    replacements: &BTreeMap<String, String>,
) -> TranslationResult<String> {
    Lang::choice(key, count, replacements, None)
}
