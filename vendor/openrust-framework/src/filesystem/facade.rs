use std::{sync::Arc, time::SystemTime};

use crate::{
    container::ServiceKey,
    http::{HeaderMap, Response, UploadedFile},
    support::facades::Facade,
};

use super::{
    FilesystemAdapter, FilesystemManager, FilesystemResult, LocalDiskConfig, LocalPermissions,
    S3DiskConfig, WriteOptions,
};

/// Resolves storage disks in the current application scope.
pub struct Storage;

impl Facade for Storage {
    type Service = FilesystemManager;

    fn get_facade_accessor() -> ServiceKey {
        "filesystem".into()
    }
}

impl Storage {
    pub async fn read_stream(path: &str) -> FilesystemResult<Option<super::FilesystemReader>> {
        Self::disk(None)?.read_stream(path).await
    }
    pub async fn write_stream(
        path: &str,
        reader: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> FilesystemResult<bool> {
        Self::disk(None)?.write_stream(path, reader).await
    }

    pub fn get_default_cloud_driver() -> FilesystemResult<String> {
        Self::get_facade_root()?.get_default_cloud_driver()
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
        Self::get_facade_root()?.disk(name)
    }

    /// 在当前应用作用域注册存储驱动。
    pub fn extend(
        driver: impl Into<String>,
        factory: impl Fn(
            &FilesystemManager,
            &serde_json::Value,
        ) -> FilesystemResult<Arc<FilesystemAdapter>>
        + Send
        + Sync
        + 'static,
    ) -> FilesystemResult<()> {
        Self::get_facade_root()?.extend(driver, factory);
        Ok(())
    }

    pub fn build_from_config(
        config: serde_json::Value,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::get_facade_root()?.build_from_config(config)
    }

    pub fn build(config: LocalDiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::get_facade_root()?.build(config)
    }

    pub fn build_with_permissions(
        config: LocalDiskConfig,
        permissions: LocalPermissions,
    ) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::get_facade_root()?.build_with_permissions(config, permissions)
    }

    pub fn build_s3(config: S3DiskConfig) -> FilesystemResult<Arc<FilesystemAdapter>> {
        Self::get_facade_root()?.build_s3(config)
    }

    pub async fn put(path: &str, contents: impl AsRef<[u8]>) -> FilesystemResult<bool> {
        Self::disk(None)?.put(path, contents).await
    }

    pub async fn put_file(
        directory: &str,
        file: &UploadedFile,
        options: WriteOptions,
    ) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.put_file(directory, file, options).await
    }

    pub async fn put_file_as(
        directory: &str,
        file: &UploadedFile,
        name: &str,
        options: WriteOptions,
    ) -> FilesystemResult<Option<String>> {
        Self::disk(None)?
            .put_file_as(directory, file, name, options)
            .await
    }

    pub async fn get(path: &str) -> FilesystemResult<Option<Vec<u8>>> {
        Self::disk(None)?.get(path).await
    }

    pub async fn size(path: &str) -> FilesystemResult<u64> {
        Self::disk(None)?.size(path).await
    }

    pub async fn last_modified(path: &str) -> FilesystemResult<SystemTime> {
        Self::disk(None)?.last_modified(path).await
    }

    pub async fn mime_type(path: &str) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.mime_type(path).await
    }

    pub async fn checksum(path: &str) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.checksum(path).await
    }

    pub async fn checksum_with<D: md5::Digest>(path: &str) -> FilesystemResult<Option<String>> {
        Self::disk(None)?.checksum_with::<D>(path).await
    }

    pub fn url(path: &str) -> FilesystemResult<String> {
        Self::disk(None)?.url(path)
    }

    pub async fn temporary_url(path: &str, expiration: SystemTime) -> FilesystemResult<String> {
        Self::disk(None)?.temporary_url(path, expiration).await
    }

    pub async fn temporary_upload_url(
        path: &str,
        expiration: SystemTime,
    ) -> FilesystemResult<super::TemporaryUploadUrl> {
        Self::disk(None)?
            .temporary_upload_url(path, expiration)
            .await
    }

    pub async fn temporary_url_with(
        path: &str,
        expiration: SystemTime,
        options: super::TemporaryUrlOptions,
    ) -> FilesystemResult<String> {
        Self::disk(None)?
            .temporary_url_with(path, expiration, options)
            .await
    }

    pub async fn temporary_upload_url_with(
        path: &str,
        expiration: SystemTime,
        options: super::TemporaryUploadUrlOptions,
    ) -> FilesystemResult<super::TemporaryUploadUrl> {
        Self::disk(None)?
            .temporary_upload_url_with(path, expiration, options)
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

    pub async fn all_files(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.all_files(directory).await
    }

    pub async fn directories(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.directories(directory).await
    }

    pub async fn all_directories(directory: Option<&str>) -> FilesystemResult<Vec<String>> {
        Self::disk(None)?.all_directories(directory).await
    }

    pub async fn make_directory(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.make_directory(path).await
    }

    pub async fn delete_directory(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.delete_directory(path).await
    }

    pub async fn delete(path: &str) -> FilesystemResult<bool> {
        Self::disk(None)?.delete(path).await
    }
}
