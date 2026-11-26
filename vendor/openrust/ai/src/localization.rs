use std::collections::BTreeMap;

use openrust::support::facades::Lang;

pub(crate) const RESOURCES: &[(&str, &str)] = &[
    ("ar/tools.json", include_str!("../lang/ar/tools.json")),
    ("de/tools.json", include_str!("../lang/de/tools.json")),
    ("en/tools.json", include_str!("../lang/en/tools.json")),
    ("es/tools.json", include_str!("../lang/es/tools.json")),
    ("fr/tools.json", include_str!("../lang/fr/tools.json")),
    ("hi/tools.json", include_str!("../lang/hi/tools.json")),
    ("id/tools.json", include_str!("../lang/id/tools.json")),
    ("it/tools.json", include_str!("../lang/it/tools.json")),
    ("ja/tools.json", include_str!("../lang/ja/tools.json")),
    ("ko/tools.json", include_str!("../lang/ko/tools.json")),
    ("nl/tools.json", include_str!("../lang/nl/tools.json")),
    ("pt/tools.json", include_str!("../lang/pt/tools.json")),
    ("ru/tools.json", include_str!("../lang/ru/tools.json")),
    ("th/tools.json", include_str!("../lang/th/tools.json")),
    ("tr/tools.json", include_str!("../lang/tr/tools.json")),
    ("vi/tools.json", include_str!("../lang/vi/tools.json")),
    (
        "zh-Hans/tools.json",
        include_str!("../lang/zh-Hans/tools.json"),
    ),
    (
        "zh-Hant/tools.json",
        include_str!("../lang/zh-Hant/tools.json"),
    ),
];

pub(crate) fn text(key: &str, replacements: &BTreeMap<String, String>, fallback: &str) -> String {
    Lang::get(key, replacements, None)
        .ok()
        .filter(|line| line != key)
        .unwrap_or_else(|| fallback.to_owned())
}
