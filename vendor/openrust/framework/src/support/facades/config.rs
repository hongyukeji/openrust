use crate::{
    config::Value,
    container::ServiceKey,
    contracts::{config::Repository, container::BindingResolutionError},
};

use super::Facade;

/// Reads the current application's configuration repository.
pub struct Config;

impl Facade for Config {
    type Service = dyn Repository;
    const cached: bool = false;

    fn getFacadeAccessor() -> ServiceKey {
        "config".into()
    }
}

impl Config {
    /// Returns the value at a dot-separated key, if it exists.
    pub fn get(key: &str) -> Result<Option<Value>, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.get(key))
    }
}
