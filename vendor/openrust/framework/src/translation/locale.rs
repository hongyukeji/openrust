use std::borrow::Cow;

use icu_locale_core::LanguageIdentifier;

use super::{TranslationError, TranslationResult};

pub(super) fn language_tag(locale: &str) -> TranslationResult<Cow<'_, str>> {
    let tag = if locale.contains('_') {
        Cow::Owned(
            LanguageIdentifier::normalize(&locale.replace('_', "-"))
                .map_err(|_| TranslationError::InvalidLocale)?
                .into_owned(),
        )
    } else {
        LanguageIdentifier::normalize(locale).map_err(|_| TranslationError::InvalidLocale)?
    };
    Ok(match tag.as_ref() {
        "zh-Hans" | "zh-Hans-CN" => Cow::Borrowed("zh-CN"),
        "zh-Hant" | "zh-Hant-TW" => Cow::Borrowed("zh-TW"),
        _ => tag,
    })
}

/// Normalizes a language identifier for application config and catalog paths.
pub fn canonical_locale(locale: &str) -> TranslationResult<String> {
    Ok(language_tag(locale)?.replace('-', "_"))
}
