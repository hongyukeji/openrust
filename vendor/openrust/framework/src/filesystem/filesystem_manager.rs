use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::Deserialize;

use crate::contracts::config::Repository;

use super::{
    AwsS3V3Adapter, FilesystemAdapter, FilesystemError, FilesystemResult, LocalFilesystemAdapter,
    S3DiskConfig, Visibility,
};

/// Resolves and caches named filesystem disks for one application.
pub struct FilesystemManager {
    config: Arc<dyn Repository>,
    base_path: PathBuf,
    disks: Mutex<HashMap<String, Arc<FilesystemAdapter>>>,
}

/// Configuration for an on-demand local disk.
#[derive(Clone, Deserialize)]
pub struct LocalDiskConfig {
    pub root: PathBuf,
    #[serde(default)]
    pub visibility: Visibility,
    pub directory_visibility: Option<Visibility>,
    #[serde(default)]
    pub throw: bool,
    #[serde(default)]
    pub report: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's filesystem manager API")]
impl FilesystemManager {
    pub fn new(config: Arc<dyn Repository>, base_path: PathBuf) -> Self {
        Self {
            config,
            base_path,
            disks: Mutex::default(),
        }
    }

    pub fn disk(&self, name: Option<&str>) -> FilesystemResult<Arc<FilesystemAdapter>> {
        let default;
        let name = match name.filter(|name| !name.is_empty()) {
            Some(name) => name,
            None => {
                default = self.getDefaultDriver()?;
                &default
            }
        };
        let mut disks = self
            .disks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(disk) = disks.get(name) {
            return Ok(Arc::clone(disk));
        }
        let config = self
            .config
            .get("filesystems.disks")
            .and_then(|disks| disks.get(name).cloned())
            .ok_or_else(|| FilesystemError::UnknownDisk { name: name.into() })?;
        let disk = match config.get("driver").and_then(serde_json::Value::as_str) {
            Some("local") => self.build(
                serde_json::from_value(config)
                    .map_err(|_| FilesystemError::InvalidConfiguration)?,
            )?,
            Some("s3") => self.buildS3(
                serde_json::from_value(config)
                    .map_err(|_| FilesystemError::InvalidConfiguration)?,
            )?,
            Some(_) => return Err(FilesystemError::UnsupportedOperation),
            None => return Err(FilesystemError::InvalidConfiguration),
        };
        disks.insert(name.to_owned(), Arc::clone(&disk));
        Ok(disk)
    }

    pub fn getDefaultDriver(&self) -> FilesystemResult<String> {
        match self.config.get("filesystems.default") {
            Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(name),
            _ => Err(FilesystemError::InvalidConfiguration),
        }
    }

    pub fn build(&self, config: LocalDiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        if config.root.as_os_str().is_empty() {
            return Err(FilesystemError::InvalidConfiguration);
        }
        let local = LocalFilesystemAdapter::new(
            self.base_path.join(config.root),
            config.visibility,
            config.directory_visibility.unwrap_or(config.visibility),
        );
        Ok(Arc::new(FilesystemAdapter::local(
            local,
            config.throw,
            config.report,
        )))
    }

    pub fn buildS3(&self, config: S3DiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        let throw = config.throw;
        let report = config.report;
        let driver = AwsS3V3Adapter::new(config)?;
        Ok(Arc::new(FilesystemAdapter::s3(driver, throw, report)))
    }

    pub fn forgetDisk(&self, name: &str) {
        self.disks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(name);
    }
}
