use std::{collections::BTreeMap, path::Path};

use crate::config::{ConfigError, Environment, Repository, Value};

pub(crate) type ConfigLoader =
    fn(&Path, &Environment) -> Result<BTreeMap<String, Value>, ConfigError>;

/// Loads application configuration into the repository.
pub struct LoadConfiguration;

impl LoadConfiguration {
    pub fn bootstrap(
        base_path: &Path,
        env: &Environment,
        load: ConfigLoader,
    ) -> Result<Repository, ConfigError> {
        let values = load(base_path, env)?;
        #[cfg(feature = "sanctum")]
        let values = crate::sanctum::with_default_configuration(values, env)?;
        Ok(Repository::new(values))
    }
}
