//! 仅用公开契约实现的外部后端；本地服务代替真实供应商。
use openrust::{
    contracts::filesystem::{Cloud, FileStream, Filesystem},
    filesystem::{
        FilesystemAdapter, FilesystemError, FilesystemFuture, FilesystemResult, Visibility,
        WriteOptions,
    },
    http::UploadedFile,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::SystemTime,
};
pub struct External {
    pub disk: Arc<FilesystemAdapter>,
    pub writes: Arc<AtomicUsize>,
}
impl Cloud for External {
    fn url(&self, path: &str) -> FilesystemResult<String> {
        Ok(format!("https://external.example/{path}"))
    }
}
impl Filesystem for External {
    fn path(&self, path: &str) -> FilesystemResult<String> {
        Ok(format!("external/{path}"))
    }
    fn exists<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::exists(self.disk.as_ref(), path)
    }
    fn file_exists<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::file_exists(self.disk.as_ref(), path)
    }
    fn directory_exists<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::directory_exists(self.disk.as_ref(), path)
    }
    fn get<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<Option<Vec<u8>>>> {
        Filesystem::get(self.disk.as_ref(), path)
    }
    fn read_stream<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<Box<dyn FileStream>>>> {
        Filesystem::read_stream(self.disk.as_ref(), path)
    }
    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: &'a [u8],
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::put(self.disk.as_ref(), path, contents, options)
    }
    fn put_file<'a>(
        &'a self,
        path: &'a str,
        file: &'a mut UploadedFile,
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<String>>> {
        Filesystem::put_file(self.disk.as_ref(), path, file, options)
    }
    fn put_file_as<'a>(
        &'a self,
        path: &'a str,
        file: &'a mut UploadedFile,
        name: &'a str,
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<String>>> {
        Filesystem::put_file_as(self.disk.as_ref(), path, file, name, options)
    }
    fn write_stream<'a>(
        &'a self,
        path: &'a str,
        stream: &'a mut dyn FileStream,
        options: &'a WriteOptions,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if path == "early" {
            return Box::pin(async { Err(FilesystemError::UnsupportedOperation) });
        }
        Filesystem::write_stream(self.disk.as_ref(), path, stream, options)
    }
    fn get_visibility<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<Visibility>> {
        Filesystem::get_visibility(self.disk.as_ref(), path)
    }
    fn set_visibility<'a>(
        &'a self,
        path: &'a str,
        visibility: Visibility,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::set_visibility(self.disk.as_ref(), path, visibility)
    }
    fn size<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<u64>> {
        Filesystem::size(self.disk.as_ref(), path)
    }
    fn last_modified<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<SystemTime>> {
        Filesystem::last_modified(self.disk.as_ref(), path)
    }
    fn mime_type<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<Option<String>>> {
        Filesystem::mime_type(self.disk.as_ref(), path)
    }
    fn delete<'a>(&'a self, paths: &'a [&'a str]) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::delete(self.disk.as_ref(), paths)
    }
    fn copy<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::copy(self.disk.as_ref(), from, to)
    }
    fn r#move<'a>(
        &'a self,
        from: &'a str,
        to: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::r#move(self.disk.as_ref(), from, to)
    }
    fn files<'a>(
        &'a self,
        directory: Option<&'a str>,
        recursive: bool,
    ) -> FilesystemFuture<'a, FilesystemResult<Vec<String>>> {
        Filesystem::files(self.disk.as_ref(), directory, recursive)
    }
    fn directories<'a>(
        &'a self,
        directory: Option<&'a str>,
        recursive: bool,
    ) -> FilesystemFuture<'a, FilesystemResult<Vec<String>>> {
        Filesystem::directories(self.disk.as_ref(), directory, recursive)
    }
    fn make_directory<'a>(&'a self, path: &'a str) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::make_directory(self.disk.as_ref(), path)
    }
    fn delete_directory<'a>(
        &'a self,
        path: &'a str,
    ) -> FilesystemFuture<'a, FilesystemResult<bool>> {
        Filesystem::delete_directory(self.disk.as_ref(), path)
    }
}
