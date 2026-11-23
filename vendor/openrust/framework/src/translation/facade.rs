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

    pub fn get_with(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::get_facade_root()?.string_with(key, args, locale)
    }

    pub fn array_with(
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        Self::get_facade_root()?.array_with(key, args, locale)
    }

    pub fn choice_with(
        key: &str,
        count: impl Into<PluralCount>,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        Self::get_facade_root()?.choice_with(key, count, args, locale)
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

    pub fn has_for_locale(key: &str, locale: Option<&str>) -> TranslationResult<bool> {
        Self::get_facade_root()?.has_for_locale(key, locale)
    }

    pub fn handle_missing_keys_using(
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.handle_missing_keys_using(handler);
        Ok(())
    }

    pub fn clear_missing_key_handler() -> TranslationResult<()> {
        Self::get_facade_root()?.clear_missing_key_handler();
        Ok(())
    }

    pub fn determine_locales_using(
        resolver: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.determine_locales_using(resolver);
        Ok(())
    }

    pub fn clear_locale_resolver() -> TranslationResult<()> {
        Self::get_facade_root()?.clear_locale_resolver();
        Ok(())
    }

    pub fn add_path(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::get_facade_root()?.add_path(path.into());
        Ok(())
    }

    pub fn add_json_path(path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::get_facade_root()?.add_json_path(path.into());
        Ok(())
    }

    pub fn add_namespace(namespace: &str, path: impl Into<PathBuf>) -> TranslationResult<()> {
        Self::get_facade_root()?.add_namespace(namespace, path.into())
    }

    pub fn load(namespace: Option<&str>, group: &str, locale: &str) -> TranslationResult<()> {
        Self::get_facade_root()?.load(namespace, group, locale)
    }

    pub async fn load_from(
        loader: &dyn Loader,
        resources: &[TranslationResource<'_>],
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.load_from(loader, resources).await
    }

    pub fn add_lines(
        lines: impl IntoIterator<Item = (String, Line)>,
        locale: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<()> {
        Self::get_facade_root()?.add_lines(lines, locale, namespace)
    }

    pub fn get_locale() -> TranslationResult<String> {
        Ok(Self::get_facade_root()?.locale())
    }

    pub fn set_locale(locale: &str) -> TranslationResult<()> {
        Self::get_facade_root()?.set_locale(locale)
    }
    pub fn get_fallback() -> TranslationResult<String> {
        Ok(Self::get_facade_root()?.get_fallback())
    }
    pub fn set_fallback(fallback: &str) -> TranslationResult<()> {
        Self::get_facade_root()?.set_fallback(fallback)
    }

    pub async fn with_locale<T>(
        locale: &str,
        operation: impl Future<Output = T>,
    ) -> TranslationResult<T> {
        Self::get_facade_root()?
            .with_locale(locale, operation)
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
