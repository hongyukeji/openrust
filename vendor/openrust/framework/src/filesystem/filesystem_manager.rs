use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use serde::Deserialize;

use crate::contracts::config::Repository;

use super::{
    AwsS3V3Adapter, FilesystemAdapter, FilesystemError, FilesystemResult, LocalFilesystemAdapter,
    LocalPermissions, S3DiskConfig, Visibility,
};

type ResolvedDisk = Arc<Mutex<Option<Arc<FilesystemAdapter>>>>;

type DriverFactory = dyn Fn(&FilesystemManager, &serde_json::Value) -> FilesystemResult<Arc<FilesystemAdapter>>
    + Send
    + Sync;

/// Resolves and caches named filesystem disks for one application.
pub struct FilesystemManager {
    config: Arc<dyn Repository>,
    base_path: PathBuf,
    disks: Mutex<HashMap<String, ResolvedDisk>>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
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

impl FilesystemManager {
    pub fn new(config: Arc<dyn Repository>, base_path: PathBuf) -> Self {
        Self {
            config,
            base_path,
            disks: Mutex::default(),
            factories: RwLock::default(),
        }
    }

    pub fn disk(&self, name: Option<&str>) -> FilesystemResult<Arc<FilesystemAdapter>> {
        let default;
        let name = match name.filter(|name| !name.is_empty()) {
            Some(name) => name,
            None => {
                default = self.get_default_driver()?;
                &default
            }
        };
        let configured = || {
            self.config
                .get("filesystems.disks")
                .and_then(|disks| disks.get(name).cloned())
                .ok_or_else(|| FilesystemError::UnknownDisk { name: name.into() })
        };
        let existing = self
            .disks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned();
        let (slot, config) = match existing {
            Some(slot) => (slot, None),
            None => {
                // 不为未知名称创建永久空槽，保留原有错误与缓存边界。
                let config = configured()?;
                let slot = self
                    .disks
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .entry(name.to_owned())
                    .or_default()
                    .clone();
                (slot, Some(config))
            }
        };
        let mut resolved = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(disk) = resolved.as_ref() {
            return Ok(Arc::clone(disk));
        }
        let config = match config {
            Some(config) => config,
            None => configured()?,
        };
        let disk = self.resolve(&config, name)?;
        *resolved = Some(Arc::clone(&disk));
        Ok(disk)
    }

    /// 注册当前应用的驱动工厂；已解析的磁盘保持不变，重建前调用 forget_disk。
    /// 工厂在注册表和全局缓存锁之外执行，可解析其他具名磁盘；不得循环解析自身。
    pub fn extend(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(&Self, &serde_json::Value) -> FilesystemResult<Arc<FilesystemAdapter>>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        self.factories
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(driver.into(), Arc::new(factory));
        self
    }

    /// 按配置构建不缓存的磁盘，保留原有强类型 local／S3 构建入口。
    pub fn build_from_config(
        &self,
        config: serde_json::Value,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        self.resolve(&config, "ondemand")
    }

    fn resolve(
        &self,
        config: &serde_json::Value,
        name: &str,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        let driver = config
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .filter(|driver| !driver.is_empty())
            .ok_or(FilesystemError::InvalidConfiguration)?;
        let factory = self
            .factories
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(driver)
            .cloned();
        if let Some(factory) = factory {
            return factory(self, config);
        }
        let url = match config.get("url") {
            Some(serde_json::Value::String(url)) if !url.is_empty() => Some(url.clone()),
            None | Some(serde_json::Value::Null) => None,
            _ => return Err(FilesystemError::InvalidConfiguration),
        };
        let serve = config.get("serve").and_then(serde_json::Value::as_bool) == Some(true);
        let disk = match config.get("driver").and_then(serde_json::Value::as_str) {
            Some("local") => {
                let permissions =
                    LocalPermissions::from_config(config.get("permissions").cloned())?;
                self.build_local(
                    serde_json::from_value(config.clone())
                        .map_err(|_| FilesystemError::InvalidConfiguration)?,
                    permissions,
                    url,
                )?
            }
            Some("s3") => self.build_s3_adapter(
                serde_json::from_value(config.clone())
                    .map_err(|_| FilesystemError::InvalidConfiguration)?,
                url,
            )?,
            Some(_) => return Err(FilesystemError::UnsupportedOperation),
            None => return Err(FilesystemError::InvalidConfiguration),
        };
        let disk = if serve {
            let mut disk = disk;
            if let Some(disk) = Arc::get_mut(&mut disk) {
                disk.serve_as(name);
            }
            disk
        } else {
            disk
        };
        Ok(disk)
    }

    pub fn get_default_driver(&self) -> FilesystemResult<String> {
        match self.config.get("filesystems.default") {
            Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(name),
            _ => Err(FilesystemError::InvalidConfiguration),
        }
    }

    pub fn get_default_cloud_driver(&self) -> FilesystemResult<String> {
        match self.config.get("filesystems.cloud") {
            None | Some(serde_json::Value::Null) => Ok("s3".into()),
            Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(name),
            _ => Err(FilesystemError::InvalidConfiguration),
        }
    }

    pub fn build(&self, config: LocalDiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        self.build_local(config, LocalPermissions::default(), None)
    }

    pub fn build_with_permissions(
        &self,
        config: LocalDiskConfig,
        permissions: LocalPermissions,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        permissions.validate()?;
        self.build_local(config, permissions, None)
    }

    fn build_local(
        &self,
        config: LocalDiskConfig,
        permissions: LocalPermissions,
        url: Option<String>,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        if config.root.as_os_str().is_empty() {
            return Err(FilesystemError::InvalidConfiguration);
        }
        let local = LocalFilesystemAdapter::new(
            self.base_path.join(config.root),
            config.visibility,
            config.directory_visibility.unwrap_or(config.visibility),
            permissions,
        );
        Ok(Arc::new(FilesystemAdapter::local(
            local,
            config.throw,
            config.report,
            url,
        )))
    }

    pub fn build_s3(&self, config: S3DiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        self.build_s3_adapter(config, None)
    }

    fn build_s3_adapter(
        &self,
        config: S3DiskConfig,
        url: Option<String>,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        let throw = config.throw;
        let report = config.report;
        let driver = AwsS3V3Adapter::new(config)?;
        Ok(Arc::new(FilesystemAdapter::s3(driver, throw, report, url)))
    }

    pub fn forget_disk(&self, name: &str) {
        self.disks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(name);
    }
}
