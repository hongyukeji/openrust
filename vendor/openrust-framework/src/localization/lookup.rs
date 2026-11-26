use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    sync::{Arc, RwLock},
};

use super::{Replacement, TranslationResult, locale::canonical_locale};

/// The context passed to the application's missing translation handler.
pub struct MissingTranslation<'a> {
    pub key: &'a str,
    pub replacements: &'a BTreeMap<String, String>,
    /// Ordered arguments from a mixed lookup, including tags and objects.
    pub mixed_replacements: &'a [(&'a str, Replacement<'a>)],
    pub locale: &'a str,
    pub fallback: bool,
}

type MissingHandler = dyn Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync;
type LocaleResolver = dyn Fn(&[String]) -> Vec<String> + Send + Sync;

#[derive(Default)]
pub(super) struct LookupHooks {
    missing: RwLock<Option<Arc<MissingHandler>>>,
    locales: RwLock<Option<Arc<LocaleResolver>>>,
}

impl LookupHooks {
    pub(super) fn set_missing(&self, handler: Option<Arc<MissingHandler>>) {
        let previous = std::mem::replace(&mut *self.missing.write().unwrap(), handler);
        drop(previous);
    }

    pub(super) fn set_locales(&self, resolver: Option<Arc<LocaleResolver>>) {
        let previous = std::mem::replace(&mut *self.locales.write().unwrap(), resolver);
        drop(previous);
    }

    pub(super) fn locales(&self, locale: &str, fallback: &str) -> TranslationResult<Vec<String>> {
        let defaults = [locale.to_owned(), fallback.to_owned()];
        let resolver = self.locales.read().unwrap().clone();
        let Some(resolver) = resolver else {
            return Ok(if locale == fallback {
                vec![locale.to_owned()]
            } else {
                defaults.into()
            });
        };
        let mut seen = HashSet::new();
        let mut ordered = Vec::new();
        for locale in resolver(&defaults) {
            let locale = canonical_locale(&locale)?;
            if seen.insert(locale.clone()) {
                ordered.push(locale);
            }
        }
        Ok(ordered)
    }

    pub(super) fn missing(&self, context: MissingTranslation<'_>) -> String {
        let handler = self.missing.read().unwrap().clone();
        let key = context.key;
        if let Some(handler) = handler
            && let Some(_guard) = MissingGuard::enter(self)
            && let Some(text) = handler(context)
        {
            return text;
        }
        key.to_owned()
    }
}

thread_local! {
    static MISSING_ACTIVE: RefCell<Vec<*const LookupHooks>> = const { RefCell::new(Vec::new()) };
}

/// Synchronous re-entry by the same localizer skips its missing-key callback.
struct MissingGuard(*const LookupHooks);

impl MissingGuard {
    fn enter(hooks: &LookupHooks) -> Option<Self> {
        MISSING_ACTIVE.with_borrow_mut(|active| {
            if active.contains(&(hooks as *const LookupHooks)) {
                return None;
            }
            active.push(hooks);
            Some(Self(hooks))
        })
    }
}

impl Drop for MissingGuard {
    fn drop(&mut self) {
        MISSING_ACTIVE.with_borrow_mut(|active| {
            let last = active.pop();
            debug_assert_eq!(last, Some(self.0));
        });
    }
}
