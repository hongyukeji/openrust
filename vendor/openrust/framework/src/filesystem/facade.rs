use std::{sync::Arc, time::SystemTime};

use crate::{
    container::ServiceKey,
    http::{HeaderMap, Response, UploadedFile},
    support::facades::Facade,
};

use super::{
    FilesystemAdapter, FilesystemError, FilesystemManager, FilesystemResult, LocalDiskConfig,
    LocalPermissions, S3DiskConfig, WriteOptions,
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
    #[allow(non_snake_case, reason = "遵循 Laravel Storage 原名")]
    pub async fn readStream(path: &str) -> FilesystemResult<Option<super::FilesystemReader>> {
        Self::disk(None)?.readStream(path).await
    }
    #[allow(non_snake_case, reason = "遵循 Laravel Storage 原名")]
    pub async fn writeStream(
        path: &str,
        reader: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> FilesystemResult<bool> {
        Self::disk(None)?.writeStream(path, reader).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub fn getDefaultCloudDriver() -> FilesystemResult<String> {
        Self::getFacadeRoot()
            .map_err(FilesystemError::Unavailable)?
            .getDefaultCloudDriver()
    }

    pub async fn copy(from: &str, to: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.copy(from, to).await
    }
    pub async fn r#move(from: &str, to: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.r#move(from, to).await
    }
    pub async fn append(path: &str, data: &str, separator: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.append(path, data, separator).await
    }
    pub async fn prepend(path: &str, data: &str, separator: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.prepend(path, data, separator).await
    }
    pub async fn json(path: &str) -> FilesystemResult<Option<serde_json::Value>> {
        Self::disk(None)?.json(path).await
    }

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
    pub fn buildWithPermissions(
        config: LocalDiskConfig,
        permissions: LocalPermissions,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::getFacadeRoot()
            .map_err(FilesystemError::Unavailable)?
            .buildWithPermissions(config, permissions)
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

    pub async fn size(path: &str) -> FilesystemResult<u64> {
        Self::disk(None)?.size(path).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn lastModified(path: &str) -> FilesystemResult<SystemTime> {
        Self::disk(None)?.lastModified(path).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn mimeType(path: &str) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.mimeType(path).await
    }

    pub async fn checksum(path: &str) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.checksum(path).await
    }

    #[allow(
        non_snake_case,
        reason = "Typed counterpart of Laravel's checksum options"
    )]
    pub async fn checksumWith<D: md5::Digest>(path: &str) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.checksumWith::<D>(path).await
    }

    pub fn url(path: &str) -> FilesystemResult<String> {
        Self::disk(None)?.url(path)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn temporaryUrl(path: &str, expiration: SystemTime) -> FilesystemResult<String> {
        Self::disk(None)?.temporaryUrl(path, expiration).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn temporaryUploadUrl(
        path: &str,
        expiration: SystemTime,
    ) -> FilesystemResult<super::TemporaryUploadUrl> {
        Self::disk(None)?.temporaryUploadUrl(path, expiration).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn temporaryUrlWith(
        path: &str,
        expiration: SystemTime,
        options: super::TemporaryUrlOptions,
    ) -> FilesystemResult<String> {
        Self::disk(None)?
            .temporaryUrlWith(path, expiration, options)
            .await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn temporaryUploadUrlWith(
        path: &str,
        expiration: SystemTime,
        options: super::TemporaryUploadUrlOptions,
    ) -> FilesystemResult<super::TemporaryUploadUrl> {
        Self::disk(None)?
            .temporaryUploadUrlWith(path, expiration, options)
            .await
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

    pub async fn files(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.files(directory).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn allFiles(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.allFiles(directory).await
    }

    pub async fn directories(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.directories(directory).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn allDirectories(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.allDirectories(directory).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn makeDirectory(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.makeDirectory(path).await
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Storage facade API")]
    pub async fn deleteDirectory(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.deleteDirectory(path).await
    }

    pub async fn delete(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.delete(path).await
    }
}
