use std::{
    ffi::OsString,
    path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path, PathBuf},
    sync::{Arc, RwLock},
};

use crate::config::Environment;

/// Shared application path state for compiled providers and path helpers.
pub struct Paths {
    base: PathBuf,
    initial_storage: Option<OsString>,
    state: RwLock<State>,
}

#[allow(
    non_snake_case,
    reason = "Preserves Laravel application path field names"
)]
#[derive(Default)]
struct State {
    storagePath: Option<PathBuf>,
    environment: Option<Arc<Environment>>,
}

impl Paths {
    pub(crate) fn new(base: PathBuf) -> Self {
        Self {
            base,
            initial_storage: std::env::var_os("APP_STORAGE_PATH"),
            state: RwLock::default(),
        }
    }

    #[allow(non_snake_case, reason = "Matches Laravel Application::storagePath")]
    pub fn storagePath(&self, path: &str) -> PathBuf {
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let override_path = state
            .storagePath
            .as_ref()
            .filter(|value| !value.as_os_str().is_empty() && value.as_os_str() != "0");
        let base = match override_path {
            Some(path) => path.clone(),
            None => {
                let value = match &state.environment {
                    Some(environment) => environment.values.get("APP_STORAGE_PATH"),
                    None => self.initial_storage.as_ref(),
                };
                value
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.base.join("storage"))
            }
        };
        join(&base, path)
    }

    pub(crate) fn use_storage_path(&self, path: PathBuf) {
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .storagePath = Some(path);
    }

    pub(crate) fn environment(&self, environment: Arc<Environment>) {
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .environment = Some(environment);
    }
}

// Laravel join_paths appends without normalizing the base or resolving traversal.
pub(crate) fn join(base: &Path, path: &str) -> PathBuf {
    if path.is_empty() {
        return base.into();
    }
    let mut result = base.as_os_str().to_owned();
    result.push(MAIN_SEPARATOR_STR);
    result.push(path.trim_start_matches(MAIN_SEPARATOR));
    result.into()
}
