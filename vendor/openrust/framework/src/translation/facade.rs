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

    fn get_facade_accessor() -> ServiceKey {
        "translator".into()
    }
}

impl Lang {
    pub fn get(
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::get_facade_root()?.string(key, replacements, locale)
    }

    pub fn choice(
        key: &str,
        count: impl Into<PluralCount>,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::get_facade_root()?.choose(key, count, replacements, locale)
    }

    pub fn array(
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::get_facade_root()?.array(key, replacements, locale)
    }

    pub fn getWith(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::get_facade_root()?.stringWith(key, args, locale)
    }

    pub fn arrayWith(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::get_facade_root()?.arrayWith(key, args, locale)
    }

    pub fn choiceWith(
        key: &str,
        count: impl Into<PluralCount>,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::get_facade_root()?.choiceWith(key, count, args, locale)
    }

    pub fn stringable<T: Any>(
        formatter: impl Fn(&T) -> String + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.stringable(formatter);
        Ok(())
    }

    pub fn has(key: &str, locale: Option<&str>, fallback: bool) -> TranslationResult<bool> {
        Self::get_facade_root()?.has(key, locale, fallback)
    }

    pub fn hasForLocale(key: &str, locale: Option<&str>) -> TranslationResult<bool> {
        Self::get_facade_root()?.hasForLocale(key, locale)
    }

    pub fn handleMissingKeysUsing(
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.handleMissingKeysUsing(handler);
        Ok(())
    }

    pub fn clearMissingKeyHandler() -> TranslationResult<()> {
        Self::get_facade_root()?.clearMissingKeyHandler();
        Ok(())
    }

    pub fn determineLocalesUsing(
        resolver: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.determineLocalesUsing(resolver);
        Ok(())
    }

    pub fn clearLocaleResolver() -> TranslationResult<()> {
        Self::get_facade_root()?.clearLocaleResolver();
        Ok(())
    }

    pub fn addPath(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::get_facade_root()?.addPath(path.into());
        Ok(())
    }

    pub fn addJsonPath(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::get_facade_root()?.addJsonPath(path.into());
        Ok(())
    }

    pub fn addNamespace(namespace: &str, path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::get_facade_root()?.addNamespace(namespace, path.into())
    }

    pub fn load(namespace: Option<&str>, group: &str, locale: &str) -> TranslationResult<()> {
        Self::get_facade_root()?.load(namespace, group, locale)
    }

    pub async fn loadFrom(
        loader: &dyn Loader,
        resources: &[TranslationResource<'_>],
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.loadFrom(loader, resources).await
    }

    pub fn addLines(
        lines: impl IntoIterator<Item = (String, Line)>,
        locale: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.addLines(lines, locale, namespace)
    }

    pub fn get_locale() -> TranslationResult<String> {
        Ok(Self::get_facade_root()?.locale())
    }

    pub fn set_locale(locale: &str) -> TranslationResult<()> {
        Self::get_facade_root()?.set_locale(locale)
    }
    pub fn getFallback() -> TranslationResult<String> {
        Ok(Self::get_facade_root()?.getFallback())
    }
    pub fn setFallback(fallback: &str) -> TranslationResult<()> {
        Self::get_facade_root()?.setFallback(fallback)
    }

    pub async fn withLocale<T>(
        locale: &str,
        operation: impl Future<Output = T>,
    ) -> TranslationResult<T> {
        Self::get_facade_root()?.withLocale(locale, operation).await
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
