// 默认语言资源随产物交付；应用语言目录只覆盖其提供的键。
use super::{Lines, TranslationError, TranslationResult};

pub(crate) const CATALOGS: &[(&str, &str)] = &[
    ("NOTICE", include_str!("stubs/NOTICE")),
    ("ar.json", include_str!("lang/ar.json")),
    ("ar/auth.json", include_str!("lang/ar/auth.json")),
    ("ar/console.json", include_str!("lang/ar/console.json")),
    ("ar/languages.json", include_str!("lang/ar/languages.json")),
    (
        "ar/pagination.json",
        include_str!("lang/ar/pagination.json"),
    ),
    ("ar/passwords.json", include_str!("lang/ar/passwords.json")),
    ("ar/schema.json", include_str!("lang/ar/schema.json")),
    (
        "ar/validation.json",
        include_str!("lang/ar/validation.json"),
    ),
    ("ar/welcome.json", include_str!("lang/ar/welcome.json")),
    ("de.json", include_str!("lang/de.json")),
    ("de/auth.json", include_str!("lang/de/auth.json")),
    ("de/console.json", include_str!("lang/de/console.json")),
    ("de/languages.json", include_str!("lang/de/languages.json")),
    (
        "de/pagination.json",
        include_str!("lang/de/pagination.json"),
    ),
    ("de/passwords.json", include_str!("lang/de/passwords.json")),
    ("de/schema.json", include_str!("lang/de/schema.json")),
    (
        "de/validation.json",
        include_str!("lang/de/validation.json"),
    ),
    ("de/welcome.json", include_str!("lang/de/welcome.json")),
    ("en.json", include_str!("lang/en.json")),
    ("en/auth.json", include_str!("lang/en/auth.json")),
    ("en/console.json", include_str!("lang/en/console.json")),
    ("en/languages.json", include_str!("lang/en/languages.json")),
    (
        "en/pagination.json",
        include_str!("lang/en/pagination.json"),
    ),
    ("en/passwords.json", include_str!("lang/en/passwords.json")),
    ("en/schema.json", include_str!("lang/en/schema.json")),
    (
        "en/validation.json",
        include_str!("lang/en/validation.json"),
    ),
    ("en/welcome.json", include_str!("lang/en/welcome.json")),
    ("es.json", include_str!("lang/es.json")),
    ("es/auth.json", include_str!("lang/es/auth.json")),
    ("es/console.json", include_str!("lang/es/console.json")),
    ("es/languages.json", include_str!("lang/es/languages.json")),
    (
        "es/pagination.json",
        include_str!("lang/es/pagination.json"),
    ),
    ("es/passwords.json", include_str!("lang/es/passwords.json")),
    ("es/schema.json", include_str!("lang/es/schema.json")),
    (
        "es/validation.json",
        include_str!("lang/es/validation.json"),
    ),
    ("es/welcome.json", include_str!("lang/es/welcome.json")),
    ("fr.json", include_str!("lang/fr.json")),
    ("fr/auth.json", include_str!("lang/fr/auth.json")),
    ("fr/console.json", include_str!("lang/fr/console.json")),
    ("fr/languages.json", include_str!("lang/fr/languages.json")),
    (
        "fr/pagination.json",
        include_str!("lang/fr/pagination.json"),
    ),
    ("fr/passwords.json", include_str!("lang/fr/passwords.json")),
    ("fr/schema.json", include_str!("lang/fr/schema.json")),
    (
        "fr/validation.json",
        include_str!("lang/fr/validation.json"),
    ),
    ("fr/welcome.json", include_str!("lang/fr/welcome.json")),
    ("hi.json", include_str!("lang/hi.json")),
    ("hi/auth.json", include_str!("lang/hi/auth.json")),
    ("hi/console.json", include_str!("lang/hi/console.json")),
    ("hi/languages.json", include_str!("lang/hi/languages.json")),
    (
        "hi/pagination.json",
        include_str!("lang/hi/pagination.json"),
    ),
    ("hi/passwords.json", include_str!("lang/hi/passwords.json")),
    ("hi/schema.json", include_str!("lang/hi/schema.json")),
    (
        "hi/validation.json",
        include_str!("lang/hi/validation.json"),
    ),
    ("hi/welcome.json", include_str!("lang/hi/welcome.json")),
    ("id.json", include_str!("lang/id.json")),
    ("id/auth.json", include_str!("lang/id/auth.json")),
    ("id/console.json", include_str!("lang/id/console.json")),
    ("id/languages.json", include_str!("lang/id/languages.json")),
    (
        "id/pagination.json",
        include_str!("lang/id/pagination.json"),
    ),
    ("id/passwords.json", include_str!("lang/id/passwords.json")),
    ("id/schema.json", include_str!("lang/id/schema.json")),
    (
        "id/validation.json",
        include_str!("lang/id/validation.json"),
    ),
    ("id/welcome.json", include_str!("lang/id/welcome.json")),
    ("it.json", include_str!("lang/it.json")),
    ("it/auth.json", include_str!("lang/it/auth.json")),
    ("it/console.json", include_str!("lang/it/console.json")),
    ("it/languages.json", include_str!("lang/it/languages.json")),
    (
        "it/pagination.json",
        include_str!("lang/it/pagination.json"),
    ),
    ("it/passwords.json", include_str!("lang/it/passwords.json")),
    ("it/schema.json", include_str!("lang/it/schema.json")),
    (
        "it/validation.json",
        include_str!("lang/it/validation.json"),
    ),
    ("it/welcome.json", include_str!("lang/it/welcome.json")),
    ("ja.json", include_str!("lang/ja.json")),
    ("ja/auth.json", include_str!("lang/ja/auth.json")),
    ("ja/console.json", include_str!("lang/ja/console.json")),
    ("ja/languages.json", include_str!("lang/ja/languages.json")),
    (
        "ja/pagination.json",
        include_str!("lang/ja/pagination.json"),
    ),
    ("ja/passwords.json", include_str!("lang/ja/passwords.json")),
    ("ja/schema.json", include_str!("lang/ja/schema.json")),
    (
        "ja/validation.json",
        include_str!("lang/ja/validation.json"),
    ),
    ("ja/welcome.json", include_str!("lang/ja/welcome.json")),
    ("ko.json", include_str!("lang/ko.json")),
    ("ko/auth.json", include_str!("lang/ko/auth.json")),
    ("ko/console.json", include_str!("lang/ko/console.json")),
    ("ko/languages.json", include_str!("lang/ko/languages.json")),
    (
        "ko/pagination.json",
        include_str!("lang/ko/pagination.json"),
    ),
    ("ko/passwords.json", include_str!("lang/ko/passwords.json")),
    ("ko/schema.json", include_str!("lang/ko/schema.json")),
    (
        "ko/validation.json",
        include_str!("lang/ko/validation.json"),
    ),
    ("ko/welcome.json", include_str!("lang/ko/welcome.json")),
    ("nl.json", include_str!("lang/nl.json")),
    ("nl/auth.json", include_str!("lang/nl/auth.json")),
    ("nl/console.json", include_str!("lang/nl/console.json")),
    ("nl/languages.json", include_str!("lang/nl/languages.json")),
    (
        "nl/pagination.json",
        include_str!("lang/nl/pagination.json"),
    ),
    ("nl/passwords.json", include_str!("lang/nl/passwords.json")),
    ("nl/schema.json", include_str!("lang/nl/schema.json")),
    (
        "nl/validation.json",
        include_str!("lang/nl/validation.json"),
    ),
    ("nl/welcome.json", include_str!("lang/nl/welcome.json")),
    ("pt.json", include_str!("lang/pt.json")),
    ("pt/auth.json", include_str!("lang/pt/auth.json")),
    ("pt/console.json", include_str!("lang/pt/console.json")),
    ("pt/languages.json", include_str!("lang/pt/languages.json")),
    (
        "pt/pagination.json",
        include_str!("lang/pt/pagination.json"),
    ),
    ("pt/passwords.json", include_str!("lang/pt/passwords.json")),
    ("pt/schema.json", include_str!("lang/pt/schema.json")),
    (
        "pt/validation.json",
        include_str!("lang/pt/validation.json"),
    ),
    ("pt/welcome.json", include_str!("lang/pt/welcome.json")),
    ("ru.json", include_str!("lang/ru.json")),
    ("ru/auth.json", include_str!("lang/ru/auth.json")),
    ("ru/console.json", include_str!("lang/ru/console.json")),
    ("ru/languages.json", include_str!("lang/ru/languages.json")),
    (
        "ru/pagination.json",
        include_str!("lang/ru/pagination.json"),
    ),
    ("ru/passwords.json", include_str!("lang/ru/passwords.json")),
    ("ru/schema.json", include_str!("lang/ru/schema.json")),
    (
        "ru/validation.json",
        include_str!("lang/ru/validation.json"),
    ),
    ("ru/welcome.json", include_str!("lang/ru/welcome.json")),
    ("th.json", include_str!("lang/th.json")),
    ("th/auth.json", include_str!("lang/th/auth.json")),
    ("th/console.json", include_str!("lang/th/console.json")),
    ("th/languages.json", include_str!("lang/th/languages.json")),
    (
        "th/pagination.json",
        include_str!("lang/th/pagination.json"),
    ),
    ("th/passwords.json", include_str!("lang/th/passwords.json")),
    ("th/schema.json", include_str!("lang/th/schema.json")),
    (
        "th/validation.json",
        include_str!("lang/th/validation.json"),
    ),
    ("th/welcome.json", include_str!("lang/th/welcome.json")),
    ("tr.json", include_str!("lang/tr.json")),
    ("tr/auth.json", include_str!("lang/tr/auth.json")),
    ("tr/console.json", include_str!("lang/tr/console.json")),
    ("tr/languages.json", include_str!("lang/tr/languages.json")),
    (
        "tr/pagination.json",
        include_str!("lang/tr/pagination.json"),
    ),
    ("tr/passwords.json", include_str!("lang/tr/passwords.json")),
    ("tr/schema.json", include_str!("lang/tr/schema.json")),
    (
        "tr/validation.json",
        include_str!("lang/tr/validation.json"),
    ),
    ("tr/welcome.json", include_str!("lang/tr/welcome.json")),
    ("vi.json", include_str!("lang/vi.json")),
    ("vi/auth.json", include_str!("lang/vi/auth.json")),
    ("vi/console.json", include_str!("lang/vi/console.json")),
    ("vi/languages.json", include_str!("lang/vi/languages.json")),
    (
        "vi/pagination.json",
        include_str!("lang/vi/pagination.json"),
    ),
    ("vi/passwords.json", include_str!("lang/vi/passwords.json")),
    ("vi/schema.json", include_str!("lang/vi/schema.json")),
    (
        "vi/validation.json",
        include_str!("lang/vi/validation.json"),
    ),
    ("vi/welcome.json", include_str!("lang/vi/welcome.json")),
    ("zh-Hans.json", include_str!("lang/zh-Hans.json")),
    ("zh-Hans/auth.json", include_str!("lang/zh-Hans/auth.json")),
    (
        "zh-Hans/console.json",
        include_str!("lang/zh-Hans/console.json"),
    ),
    (
        "zh-Hans/languages.json",
        include_str!("lang/zh-Hans/languages.json"),
    ),
    (
        "zh-Hans/pagination.json",
        include_str!("lang/zh-Hans/pagination.json"),
    ),
    (
        "zh-Hans/passwords.json",
        include_str!("lang/zh-Hans/passwords.json"),
    ),
    (
        "zh-Hans/schema.json",
        include_str!("lang/zh-Hans/schema.json"),
    ),
    (
        "zh-Hans/validation.json",
        include_str!("lang/zh-Hans/validation.json"),
    ),
    (
        "zh-Hans/welcome.json",
        include_str!("lang/zh-Hans/welcome.json"),
    ),
    ("zh-Hant.json", include_str!("lang/zh-Hant.json")),
    ("zh-Hant/auth.json", include_str!("lang/zh-Hant/auth.json")),
    (
        "zh-Hant/console.json",
        include_str!("lang/zh-Hant/console.json"),
    ),
    (
        "zh-Hant/languages.json",
        include_str!("lang/zh-Hant/languages.json"),
    ),
    (
        "zh-Hant/pagination.json",
        include_str!("lang/zh-Hant/pagination.json"),
    ),
    (
        "zh-Hant/passwords.json",
        include_str!("lang/zh-Hant/passwords.json"),
    ),
    (
        "zh-Hant/schema.json",
        include_str!("lang/zh-Hant/schema.json"),
    ),
    (
        "zh-Hant/validation.json",
        include_str!("lang/zh-Hant/validation.json"),
    ),
    (
        "zh-Hant/welcome.json",
        include_str!("lang/zh-Hant/welcome.json"),
    ),
];

pub(super) fn load(locale: &str, group: &str) -> TranslationResult<Lines> {
    let name = if group == "*" {
        format!("{locale}.json")
    } else {
        format!("{locale}/{group}.json")
    };
    CATALOGS.iter().find(|(path, _)| *path == name).map_or_else(
        || Ok(Lines::new()),
        |(_, contents)| {
            serde_json::from_str(contents)
                .map_err(|source| TranslationError::Source(Box::new(source)))
        },
    )
}

pub(super) fn locales() -> impl Iterator<Item = &'static str> {
    CATALOGS
        .iter()
        .filter_map(|(name, _)| name.strip_suffix("/languages.json"))
}
