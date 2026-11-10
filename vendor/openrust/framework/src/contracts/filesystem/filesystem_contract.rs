use std::time::SystemTime;

use crate::{
    filesystem::{FilesystemFuture, FilesystemResult, Visibility, WriteOptions},
    http::UploadedFile,
};

use super::FileStream;

/// Reads and writes files relative to a disk root.
pub trait Filesystem: Send + Sync {
    fn path(&self, path: &str) -> FilesystemResult<String>;
    fn exists<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn get<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<Option<Vec<u8>>>>;
    fn read_stream<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<Box<dyn FileStream>>>>;
    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: &'a [u8],
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn put_file<'a>(
        &'a self,
        directory: &'a str,
        file: &'a mut UploadedFile,
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<String>>>;
    fn put_file_as<'a>(
        &'a self,
        directory: &'a str,
        file: &'a mut UploadedFile,
        name: &'a str,
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<String>>>;
    fn write_stream<'a>(
        &'a self,
        path: &'a str,
        stream: &'a mut dyn FileStream,
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn get_visibility<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<Visibility>>;
    fn set_visibility<'a>(
        &'a self,
        path: &'a str,
        visibility: Visibility,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn size<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<u64>>;
    fn last_modified<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<SystemTime>>;
    fn delete<'a>(&'a self, paths: &'a [&'a str]) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn copy<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn r#move<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn files<'a>(
        &'a self,
        directory: Option<&'a str>,
        recursive: bool,
    ) -> FilesystemFuture<'a, FilesystemResult<Vec<String>>>;
    fn directories<'a>(
        &'a self,
        directory: Option<&'a str>,
        recursive: bool,
    ) -> FilesystemFuture<'a, FilesystemResult<Vec<String>>>;
    fn make_directory<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<bool>>;
    fn delete_directory<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>>;
}
