use openrust::support::facades::Lang;

pub(crate) const NAMESPACE: &str = "ai-gateway";

macro_rules! resources {
    ($($locale:literal),* $(,)?) => {
        &[
            $(
                (concat!($locale, "/console.json"), include_str!(concat!("../lang/", $locale, "/console.json"))),
                (concat!($locale, "/errors.json"), include_str!(concat!("../lang/", $locale, "/errors.json"))),
            )*
        ]
    };
}

pub(crate) const RESOURCES: &[(&str, &str)] = resources!(
    "ar", "de", "en", "es", "fr", "hi", "id", "it", "ja", "ko", "nl", "pt", "ru", "th", "tr", "vi",
    "zh-Hans", "zh-Hant",
);

pub(crate) fn error(key: &str, fallback: &str) -> String {
    let key = format!("{NAMESPACE}::errors.{key}");
    Lang::get(&key, &Default::default(), None)
        .ok()
        .filter(|line| line != &key)
        .unwrap_or_else(|| fallback.to_owned())
}
