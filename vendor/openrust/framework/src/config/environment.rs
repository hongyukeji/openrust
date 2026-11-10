use std::{collections::HashMap, ffi::OsString};

use super::ConfigError;

/// The immutable environment snapshot for one application bootstrap.
pub struct Environment {
    pub(crate) values: HashMap<String, OsString>,
}

impl Environment {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(|value| value.to_str())
    }

    pub fn get_utf8(&self, key: &str) -> Result<Option<&str>, ConfigError> {
        self.values
            .get(key)
            .map(|value| {
                value.to_str().ok_or_else(|| ConfigError::InvalidType {
                    key: key.to_owned(),
                    expected: "valid UTF-8",
                })
            })
            .transpose()
    }
}
