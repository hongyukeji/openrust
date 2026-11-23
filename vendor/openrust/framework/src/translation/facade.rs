#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{any::Any, collections::BTreeMap, future::Future, path::PathBuf};

use crate::{container::ServiceKey, contracts::translation::Loader, support::facades::Facade};

use super::{
    Line, Lines, MissingTranslation, PluralCount, Replacement, TranslationResource,
    TranslationResult, Translator,
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
        Self::getFacadeRoot()?.string(key, replacements, locale)
    }

    pub fn choice(
        key: &str,
        count: impl Into<PluralCount>,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()?.choose(key, count, replacements, locale)
    }

    pub fn array(
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::getFacadeRoot()?.array(key, replacements, locale)
    }

    pub fn getWith(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()?.stringWith(key, args, locale)
    }

    pub fn arrayWith(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::getFacadeRoot()?.arrayWith(key, args, locale)
    }

    pub fn choiceWith(
        key: &str,
        count: impl Into<PluralCount>,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::getFacadeRoot()?.choiceWith(key, count, args, locale)
    }

    pub fn stringable<T: Any>(
        formatter: impl Fn(&T) -> String + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()?.stringable(formatter);
        Ok(())
    }

    pub fn has(key: &str, locale: Option<&str>, fallback: bool) -> TranslationResult<bool> {
        Self::getFacadeRoot()?.has(key, locale, fallback)
    }

    pub fn hasForLocale(key: &str, locale: Option<&str>) -> TranslationResult<bool> {
        Self::getFacadeRoot()?.hasForLocale(key, locale)
    }

    pub fn handleMissingKeysUsing(
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()?.handleMissingKeysUsing(handler);
        Ok(())
    }

    pub fn clearMissingKeyHandler() -> TranslationResult<()> {
        Self::getFacadeRoot()?.clearMissingKeyHandler();
        Ok(())
    }

    pub fn determineLocalesUsing(
        resolver: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()?.determineLocalesUsing(resolver);
        Ok(())
    }

    pub fn clearLocaleResolver() -> TranslationResult<()> {
        Self::getFacadeRoot()?.clearLocaleResolver();
        Ok(())
    }

    pub fn addPath(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::getFacadeRoot()?.addPath(path.into());
        Ok(())
    }

    pub fn addJsonPath(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::getFacadeRoot()?.addJsonPath(path.into());
        Ok(())
    }

    pub fn addNamespace(namespace: &str, path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::getFacadeRoot()?.addNamespace(namespace, path.into())
    }

    pub fn load(namespace: Option<&str>, group: &str, locale: &str) -> TranslationResult<()> {
        Self::getFacadeRoot()?.load(namespace, group, locale)
    }

    pub async fn loadFrom(
        loader: &dyn Loader,
        resources: &[TranslationResource<'_>],
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()?.loadFrom(loader, resources).await
    }

    pub fn addLines(
        lines: impl IntoIterator<Item = (String, Line)>,
        locale: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<()> {
        Self::getFacadeRoot()?.addLines(lines, locale, namespace)
    }

    pub fn getLocale() -> TranslationResult<String> {
        Ok(Self::getFacadeRoot()?.locale())
    }

    pub fn setLocale(locale: &str) -> TranslationResult<()> {
        Self::getFacadeRoot()?.setLocale(locale)
    }
    pub fn getFallback() -> TranslationResult<String> {
        Ok(Self::getFacadeRoot()?.getFallback())
    }
    pub fn setFallback(fallback: &str) -> TranslationResult<()> {
        Self::getFacadeRoot()?.setFallback(fallback)
    }

    pub async fn withLocale<T>(
        locale: &str,
        operation: impl Future<Output = T>,
    ) -> TranslationResult<T> {
        Self::getFacadeRoot()?.withLocale(locale, operation).await
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
