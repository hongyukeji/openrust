use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    FilesystemAdapter, FilesystemError, FilesystemManager, FilesystemResult, LocalDiskConfig,
};

/// Resolves storage disks in the current application scope.
pub struct Storage;

impl Facade for Storage {
    type Service = FilesystemManager;

    fn getFacadeAccessor() -> ServiceKey {
        "filesystem".into()
    }
}

impl Storage {
    pub fn disk(name: Option<&str>) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::getFacadeRoot()
            .map_err(FilesystemError::Unavailable)?
            .disk(name)
    }

    pub fn build(config: LocalDiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::getFacadeRoot()
            .map_err(FilesystemError::Unavailable)?
            .build(config)
    }

    pub async fn put(path: &str, contents: impl AsRef<[u8]>) -> FilesystemResult<bool> {
        Self::disk(None)?.put(path, contents).await
    }

    pub async fn get(path: &str) -> FilesystemResult<Option<Vec<u8>>> {
        Self::disk(None)?.get(path).await
    }

    pub async fn exists(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.exists(path).await
    }

    pub async fn delete(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.delete(path).await
    }
}
