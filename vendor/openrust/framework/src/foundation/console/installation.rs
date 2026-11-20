//! Shared package resource installation under the application's source mutex.
use crate::{
    console::{ConsoleError, ConsoleFuture, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};
use std::{
    collections::BTreeMap,
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
};

pub type InstallResources = for<'a> fn(
    &'a SourceInstallation<'a>,
    InstallationOptions<'a>,
    &'a mut Output,
) -> ConsoleFuture<'a, Result<(), ConsoleError>>;

#[derive(Clone, Copy)]
pub struct InstallationOptions<'a> {
    pub force: bool,
    pub extensions: &'a [String],
}

#[derive(Default)]
pub struct ExtensionInstallers(Mutex<BTreeMap<&'static str, InstallResources>>);
impl ExtensionInstallers {
    pub fn register(&self, name: &'static str, installer: InstallResources) {
        self.0
            .lock()
            .expect("extension installers lock")
            .insert(name, installer);
    }
    pub(crate) fn get(&self, name: &str) -> Option<InstallResources> {
        self.0
            .lock()
            .expect("extension installers lock")
            .get(name)
            .copied()
    }
}

/// Owns the same fail-fast source lock as the individual Artisan installers.
/// Package services receive this capability instead of acquiring nested locks.
pub struct SourceInstallation<'a> {
    app: &'a Application,
    _lock: File,
}
impl<'a> SourceInstallation<'a> {
    pub fn new(app: &'a Application) -> Result<Self, ConsoleError> {
        Ok(Self {
            app,
            _lock: super::source_edit::lock_manifest(app.base_path())?,
        })
    }
    pub(crate) fn from_lock(app: &'a Application, lock: File) -> Self {
        Self { app, _lock: lock }
    }
    pub(crate) fn into_lock(self) -> File {
        self._lock
    }
    pub fn application(&self) -> &Application {
        self.app
    }
    pub fn root(&self) -> &Path {
        self.app.base_path()
    }
    /// Wire an explicitly published `config/{name}.rs` with a `load(env)` entry.
    /// Uses the installation's existing source lock and preserves existing hooks.
    #[allow(
        non_snake_case,
        reason = "Matches the compiled configuration registration API"
    )]
    pub fn registerConfiguration(&self, name: &str) -> Result<(), ConsoleError> {
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            })
        {
            return Err(ConsoleError::new(
                "Configuration module must be a Rust identifier.",
            ));
        }
        let path = self.root().join("config/mod.rs");
        if !self.root().join(format!("config/{name}.rs")).is_file() {
            return Err(ConsoleError::new(format!(
                "Publish config/{name}.rs before registering its loader."
            )));
        }
        let source = super::source_edit::read(&path)?;
        let mut updated = source.clone();
        if !source.lines().any(|line| {
            [
                format!("mod {name};"),
                format!("pub mod {name};"),
                format!("pub(crate) mod {name};"),
            ]
            .iter()
            .any(|declaration| line.trim() == declaration)
        }) {
            updated = super::source_edit::insert_before(
                &updated,
                &path,
                &format!("mod {name};"),
                |line| line.trim() == "mod services;",
            )?;
        }
        let compact: String = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        if !compact.contains(&format!("(\"{name}\".")) {
            updated = super::source_edit::insert_before(
                &updated,
                &path,
                &format!("(\"{name}\".to_owned(), {name}::load(env)?),"),
                |line| line.trim_start().starts_with("(\"services\".to_owned(),"),
            )?;
        }
        super::source_edit::replace(&path, &source, &updated)
    }

    pub async fn publish(
        &self,
        tag: &str,
        force: bool,
        output: &mut Output,
    ) -> Result<(), ConsoleError> {
        super::vendor_publish_command::publish_installation(self.app, tag, force, output).await
    }
}

pub(super) fn bind(application: &Arc<super::super::application::ApplicationState>) {
    application
        .container
        .instance(Arc::new(ExtensionInstallers::default()))
        .expect("extension installers initially unbound");
}
