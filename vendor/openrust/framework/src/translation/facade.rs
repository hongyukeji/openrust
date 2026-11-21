#![allow(non_snake_case, reason = "Matches Laravel's Lang API")]

use std::{any::Any, collections::BTreeMap, future::Future, path::PathBuf};

use crate::{container::ServiceKey, contracts::translation::Loader, support::facades::Facade};

use super::{
    Line, Lines, MissingTranslation, PluralCount, Replacement, TranslationError,
    TranslationResource, TranslationResult, Translator,
};

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

    pub fn getWith(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .stringWith(key, args, locale)
    }

    pub fn arrayWith(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .arrayWith(key, args, locale)
    }

    pub fn choiceWith(
        key: &str,
        count: impl Into<PluralCount>,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .choiceWith(key, count, args, locale)
    }

    pub fn stringable<T: Any>(
        formatter: impl Fn(&T) -> String + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .stringable(formatter);
        Ok(())
    }

    pub fn has(key: &str, locale: Option<&str>, fallback: bool) -> TranslationResult<bool> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .has(key, locale, fallback)
    }

    pub fn hasForLocale(key: &str, locale: Option<&str>) -> TranslationResult<bool> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .hasForLocale(key, locale)
    }

    pub fn handleMissingKeysUsing(
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .handleMissingKeysUsing(handler);
        Ok(())
    }

    pub fn clearMissingKeyHandler() -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .clearMissingKeyHandler();
        Ok(())
    }

    pub fn determineLocalesUsing(
        resolver: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .determineLocalesUsing(resolver);
        Ok(())
    }

    pub fn clearLocaleResolver() -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .clearLocaleResolver();
        Ok(())
    }

    pub fn addPath(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .addPath(path.into());
        Ok(())
    }

    pub fn addJsonPath(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .addJsonPath(path.into());
        Ok(())
    }

    pub fn addNamespace(namespace: &str, path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .addNamespace(namespace, path.into())
    }

    pub fn load(namespace: Option<&str>, group: &str, locale: &str) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .load(namespace, group, locale)
    }

    pub async fn loadFrom(
        loader: &dyn Loader,
        resources: &[TranslationResource<'_>],
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .loadFrom(loader, resources)
            .await
    }

    pub fn addLines(
        lines: impl IntoIterator<Item = (String, Line)>,
        locale: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .addLines(lines, locale, namespace)
    }

    pub fn getLocale() -> TranslationResult<String> {
        Ok(Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .locale())
    }

    pub fn setLocale(locale: &str) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .setLocale(locale)
    }
    pub fn getFallback() -> TranslationResult<String> {
        Ok(Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .getFallback())
    }
    pub fn setFallback(fallback: &str) -> TranslationResult<()> {
        Self::getFacadeRoot()
            .map_err(TranslationError::Unavailable)?
            .setFallback(fallback)
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
