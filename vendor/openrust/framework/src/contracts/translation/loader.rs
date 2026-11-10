use crate::translation::{Lines, TranslationFuture, TranslationResult};

/// Loads a translation group for a locale and optional package namespace.
pub trait Loader: Send + Sync {
    fn load<'a>(
        &'a self,
        locale: &'a str,
        group: &'a str,
        namespace: Option<&'a str>,
    ) -> TranslationFuture<'a, TranslationResult<Lines>>;
}
