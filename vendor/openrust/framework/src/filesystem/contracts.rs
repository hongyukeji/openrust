use super::{
    FilesystemAdapter as Adapter, FilesystemFuture as Future, FilesystemManager,
    FilesystemResult as Result, Visibility, WriteOptions,
};
use crate::{
    contracts::filesystem::{Cloud, Factory, FileStream, Filesystem},
    http::UploadedFile,
};
use std::{sync::Arc, time::SystemTime};
use tokio::io::AsyncReadExt as _;

impl Factory for FilesystemManager {
    fn disk<'a>(&'a self, name: Option<&'a str>) -> Future<'a, Result<Arc<Adapter>>> {
        Box::pin(async move { Self::disk(self, name) })
    }
}
impl Cloud for Adapter {
    fn url(&self, path: &str) -> Result<String> {
        Self::url(self, path)
    }
}
struct Reader(super::FilesystemReader);
impl FileStream for Reader {
    fn next_chunk(&mut self) -> Future<'_, Result<Option<Vec<u8>>>> {
        Box::pin(async {
            let mut bytes = vec![0; 64 * 1024];
            let n = self.0.read(&mut bytes).await?;
            bytes.truncate(n);
            Ok((n > 0).then_some(bytes))
        })
    }
}
impl Filesystem for Adapter {
    fn path(&self, path: &str) -> Result<String> {
        Self::path(self, path)?
            .into_os_string()
            .into_string()
            .map_err(|_| super::FilesystemError::InvalidPath)
    }
    fn exists<'a>(&'a self, path: &'a str) -> Future<'a, Result<bool>> {
        Box::pin(Self::exists(self, path))
    }
    fn get<'a>(&'a self, path: &'a str) -> Future<'a, Result<Option<Vec<u8>>>> {
        Box::pin(Self::get(self, path))
    }
    fn read_stream<'a>(&'a self, path: &'a str) -> Future<'a, Result<Option<Box<dyn FileStream>>>> {
        Box::pin(async move {
            Ok(Self::readStream(self, path)
                .await?
                .map(|r| Box::new(Reader(r)) as Box<dyn FileStream>))
        })
    }
    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: &'a [u8],
        options: &'a WriteOptions,
    ) -> Future<'a, Result<bool>> {
        Box::pin(Self::putWithOptions(self, path, contents, *options))
    }
    fn put_file<'a>(
        &'a self,
        path: &'a str,
        file: &'a mut UploadedFile,
        options: &'a WriteOptions,
    ) -> Future<'a, Result<Option<String>>> {
        Box::pin(Self::putFile(self, path, file, *options))
    }
    fn put_file_as<'a>(
        &'a self,
        path: &'a str,
        file: &'a mut UploadedFile,
        name: &'a str,
        options: &'a WriteOptions,
    ) -> Future<'a, Result<Option<String>>> {
        Box::pin(Self::putFileAs(self, path, file, name, *options))
    }
    fn write_stream<'a>(
        &'a self,
        path: &'a str,
        stream: &'a mut dyn FileStream,
        options: &'a WriteOptions,
    ) -> Future<'a, Result<bool>> {
        Box::pin(async move {
            let chunks = futures_util::stream::try_unfold(stream, |stream| async {
                stream
                    .next_chunk()
                    .await
                    .map_err(std::io::Error::other)
                    .map(|chunk| chunk.map(|bytes| (axum::body::Bytes::from(bytes), stream)))
            });
            let mut reader = tokio_util::io::StreamReader::new(Box::pin(chunks));
            Self::writeStreamWithOptions(self, path, &mut reader, *options).await
        })
    }
    fn get_visibility<'a>(&'a self, path: &'a str) -> Future<'a, Result<Visibility>> {
        Box::pin(Self::getVisibility(self, path))
    }
    fn set_visibility<'a>(
        &'a self,
        path: &'a str,
        visibility: Visibility,
    ) -> Future<'a, Result<bool>> {
        Box::pin(Self::setVisibility(self, path, visibility))
    }
    fn size<'a>(&'a self, path: &'a str) -> Future<'a, Result<u64>> {
        Box::pin(Self::size(self, path))
    }
    fn last_modified<'a>(&'a self, path: &'a str) -> Future<'a, Result<SystemTime>> {
        Box::pin(Self::lastModified(self, path))
    }
    fn delete<'a>(&'a self, paths: &'a [&'a str]) -> Future<'a, Result<bool>> {
        Box::pin(async move {
            let mut result = true;
            for path in paths {
                result &= Self::delete(self, path).await?;
            }
            Ok(result)
        })
    }
    fn copy<'a>(&'a self, from: &'a str, to: &'a str) -> Future<'a, Result<bool>> {
        Box::pin(Self::copy(self, from, to))
    }
    fn r#move<'a>(&'a self, from: &'a str, to: &'a str) -> Future<'a, Result<bool>> {
        Box::pin(Self::r#move(self, from, to))
    }
    fn files<'a>(
        &'a self,
        path: Option<&'a str>,
        recursive: bool,
    ) -> Future<'a, Result<Vec<String>>> {
        Box::pin(async move {
            if recursive {
                Self::all_files(self, path).await
            } else {
                Self::files(self, path).await
            }
        })
    }
    fn directories<'a>(
        &'a self,
        path: Option<&'a str>,
        recursive: bool,
    ) -> Future<'a, Result<Vec<String>>> {
        Box::pin(async move {
            if recursive {
                Self::allDirectories(self, path).await
            } else {
                Self::directories(self, path).await
            }
        })
    }
    fn make_directory<'a>(&'a self, path: &'a str) -> Future<'a, Result<bool>> {
        Box::pin(Self::makeDirectory(self, path))
    }
    fn delete_directory<'a>(&'a self, path: &'a str) -> Future<'a, Result<bool>> {
        Box::pin(Self::deleteDirectory(self, path))
    }
}
