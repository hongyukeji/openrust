pub(crate) const NAMESPACE: &str = "ai-gateway";

macro_rules! resources {
    ($($locale:literal),* $(,)?) => {
        &[
            $(
                (concat!($locale, "/console.json"), include_str!(concat!("../lang/", $locale, "/console.json"))),
            )*
        ]
    };
}

pub(crate) const RESOURCES: &[(&str, &str)] = resources!(
    "ar", "de", "en", "es", "fr", "hi", "id", "it", "ja", "ko", "nl", "pt", "ru", "th", "tr", "vi",
    "zh-Hans", "zh-Hant",
);
