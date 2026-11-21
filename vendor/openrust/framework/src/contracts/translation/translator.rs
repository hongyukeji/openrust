#![allow(non_snake_case, reason = "遵循 Laravel 翻译契约原名")]

use std::collections::BTreeMap;

use crate::translation::{Lines, TranslationResult};

/// Resolves messages using the selected or fallback locale.
pub trait Translator: Send + Sync {
    fn get(
        &self,
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> String;

    fn choice(
        &self,
        key: &str,
        count: u64,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> String;

    fn array(
        &self,
        key: &str,
        replacements: &BTreeMap<String, String>,
        locale: Option<&str>,
    ) -> TranslationResult<Lines>;

    fn getLocale(&self) -> String;
    fn setLocale(&self, locale: &str) -> TranslationResult<()>;
    fn getFallback(&self) -> String;
    fn setFallback(&self, fallback: &str) -> TranslationResult<()>;
}
