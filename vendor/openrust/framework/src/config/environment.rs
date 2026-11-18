use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Weak,
};

use super::ConfigError;

/// The immutable environment snapshot for one application bootstrap.
pub struct Environment {
    pub(crate) paths: Weak<crate::foundation::Paths>,
    pub(crate) file: PathBuf,
    pub(crate) values: HashMap<String, OsString>,
}

impl Environment {
    /// Resolves configuration paths using this snapshot's owning application.
    /// Standalone snapshots fall back to their own environment and supplied base.
    pub fn storage_path(&self, base: &Path, path: &str) -> PathBuf {
        let root = self.paths.upgrade().map_or_else(
            || {
                self.values
                    .get("APP_STORAGE_PATH")
                    .map_or_else(|| base.join("storage"), PathBuf::from)
            },
            |paths| paths.storagePath(""),
        );
        crate::foundation::paths::join(&root, path)
    }

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
