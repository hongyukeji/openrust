use std::collections::BTreeMap;

use openrust::support::facades::Lang;

macro_rules! resources {
    ($($locale:literal),* $(,)?) => {
        &[
            $(
                (concat!($locale, "/tools.json"), include_str!(concat!("../lang/", $locale, "/tools.json"))),
                (concat!($locale, "/console.json"), include_str!(concat!("../lang/", $locale, "/console.json"))),
            )*
        ]
    };
}

pub(crate) const RESOURCES: &[(&str, &str)] = resources!(
    "ar", "de", "en", "es", "fr", "hi", "id", "it", "ja", "ko", "nl", "pt", "ru", "th", "tr", "vi",
    "zh-Hans", "zh-Hant"
);

pub(crate) fn text(key: &str, replacements: &BTreeMap<String, String>, fallback: &str) -> String {
    Lang::get(key, replacements, None)
        .ok()
        .filter(|line| line != key)
        .unwrap_or_else(|| fallback.to_owned())
}
