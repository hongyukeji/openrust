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

    fn get_locale(&self) -> &str;
    fn set_locale(&mut self, locale: &str);
}
