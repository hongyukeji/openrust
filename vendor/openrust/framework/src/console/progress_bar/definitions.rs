use super::{ConsoleError, ProgressFormatter};
use crate::contracts::container::Container as _;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
#[derive(Default)]
pub(crate) struct Definitions {
    formats: RwLock<BTreeMap<String, String>>,
    placeholders: RwLock<BTreeMap<String, ProgressFormatter>>,
}
impl Definitions {
    pub fn set_format(&self, name: &str, format: &str) {
        self.formats
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.to_owned(), format.to_owned());
    }
    pub fn get_format(&self, name: &str) -> Option<String> {
        self.formats
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }
    pub fn set_placeholder(&self, name: &str, callback: ProgressFormatter) {
        let retired = self
            .placeholders
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.to_owned(), callback);
        drop(retired);
    }
    pub fn get_placeholder(&self, name: &str) -> Option<ProgressFormatter> {
        self.placeholders
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }
}
pub(super) fn current(required: bool) -> Result<Option<Arc<Definitions>>, ConsoleError> {
    if let Some(container) = crate::support::facades::current_container()
        && container.bound::<Definitions>()
    {
        return container
            .make::<Definitions>()
            .map(Some)
            .map_err(ConsoleError::from_source);
    }
    if required {
        Err(ConsoleError::from_source(
            super::ConsoleProgressError::MissingDefinitions,
        ))
    } else {
        Ok(None)
    }
}
