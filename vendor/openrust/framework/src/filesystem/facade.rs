use std::sync::Arc;

use crate::{
    container::ServiceKey,
    http::{HeaderMap, Response, UploadedFile},
    support::facades::Facade,
};

use super::{
    FilesystemAdapter, FilesystemError, FilesystemManager, FilesystemResult, LocalDiskConfig,
    S3DiskConfig, WriteOptions,
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

    #[allow(
        non_snake_case,
        reason = "Typed counterpart of Laravel's Storage::build"
    )]
    pub fn buildS3(config: S3DiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::getFacadeRoot()
            .map_err(FilesystemError::Unavailable)?
            .buildS3(config)
    }

    pub async fn put(path: &str, contents: impl AsRef<[u8]>) -> FilesystemResult<bool> {
        Self::disk(None)?.put(path, contents).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn putFile(
        directory: &str,
        file: &UploadedFile,
        options: WriteOptions,
    ) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.putFile(directory, file, options).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn putFileAs(
        directory: &str,
        file: &UploadedFile,
        name: &str,
        options: WriteOptions,
    ) -> FilesystemResult<Option<String>> {
        Self::disk(None)?
            .putFileAs(directory, file, name, options)
            .await
    }

    pub async fn get(path: &str) -> FilesystemResult<Option<Vec<u8>>> {
        Self::disk(None)?.get(path).await
    }

    pub fn url(path: &str) -> FilesystemResult<String> {
        Self::disk(None)?.url(path)
    }

    pub async fn download(
        path: &str,
        name: Option<&str>,
        headers: HeaderMap,
    ) -> FilesystemResult<Response> {
        Self::disk(None)?.download(path, name, headers).await
    }

    pub async fn exists(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.exists(path).await
    }

    pub async fn delete(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.delete(path).await
    }
}
