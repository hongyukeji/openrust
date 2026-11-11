use std::path::PathBuf;

use tokio::io::{AsyncRead, AsyncReadExt as _};

use super::{
    AwsS3V3Adapter, FilesystemError, FilesystemReader, FilesystemResult, LocalFilesystemAdapter,
    Visibility, WriteOptions, is_staging_name,
};

/// Applies disk configuration and exposes filesystem operations.
pub struct FilesystemAdapter {
    driver: Driver,
    throw: bool,
    report: bool,
}

enum Driver {
    Local(LocalFilesystemAdapter),
    S3(Box<AwsS3V3Adapter>),
}

#[allow(non_snake_case, reason = "Matches Laravel's filesystem adapter API")]
impl FilesystemAdapter {
    pub(super) fn local(local: LocalFilesystemAdapter, throw: bool, report: bool) -> Self {
        Self {
            driver: Driver::Local(local),
            throw,
            report,
        }
    }

    pub(super) fn s3(driver: AwsS3V3Adapter, throw: bool, report: bool) -> Self {
        Self {
            driver: Driver::S3(Box::new(driver)),
            throw,
            report,
        }
    }

    pub fn path(&self, path: &str) -> FilesystemResult<PathBuf> {
        validate_path(path)?;
        Ok(match &self.driver {
            Driver::Local(disk) => disk.root().join(path),
            Driver::S3(disk) => PathBuf::from(disk.key(path)),
        })
    }

    pub async fn get(&self, path: &str) -> FilesystemResult<Option<Vec<u8>>> {
        let Some(mut reader) = self.readStream(path).await? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        let result = reader.read_to_end(&mut bytes).await.map(|_| Some(bytes));
        self.or_default(result.map_err(FilesystemError::from))
    }

    pub async fn readStream(&self, path: &str) -> FilesystemResult<Option<FilesystemReader>> {
        validate_path(path)?;
        let result = match &self.driver {
            Driver::Local(disk) => disk
                .read(path.into())
                .await
                .map(|reader| Box::new(reader) as FilesystemReader),
            Driver::S3(disk) => disk.read(path).await,
        };
        self.or_default(result.map(Some))
    }

    pub async fn put(&self, path: &str, contents: impl AsRef<[u8]>) -> FilesystemResult<bool> {
        self.putWithOptions(path, contents, WriteOptions::default())
            .await
    }

    pub async fn putWithOptions(
        &self,
        path: &str,
        contents: impl AsRef<[u8]>,
        options: WriteOptions,
    ) -> FilesystemResult<bool> {
        self.writeStreamWithOptions(path, &mut contents.as_ref(), options)
            .await
    }

    pub async fn writeStream(
        &self,
        path: &str,
        reader: &mut (impl AsyncRead + Unpin),
    ) -> FilesystemResult<bool> {
        self.writeStreamWithOptions(path, reader, WriteOptions::default())
            .await
    }

    pub async fn writeStreamWithOptions(
        &self,
        path: &str,
        reader: &mut (impl AsyncRead + Unpin),
        options: WriteOptions,
    ) -> FilesystemResult<bool> {
        validate_path(path)?;
        let result = match &self.driver {
            Driver::Local(disk) => disk.write(path.into(), reader, options.visibility).await,
            Driver::S3(disk) => disk.write(path, reader, options.visibility).await,
        };
        self.or_default(result.map(|()| true))
    }

    pub async fn exists(&self, path: &str) -> FilesystemResult<bool> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(disk) => disk.exists(path.into(), None).await,
            Driver::S3(disk) if disk.file_exists(path).await? => Ok(true),
            Driver::S3(disk) => disk.directory_exists(path).await,
        }
    }

    pub async fn missing(&self, path: &str) -> FilesystemResult<bool> {
        Ok(!self.exists(path).await?)
    }

    pub async fn fileExists(&self, path: &str) -> FilesystemResult<bool> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(disk) => disk.exists(path.into(), Some(false)).await,
            Driver::S3(disk) => disk.file_exists(path).await,
        }
    }

    pub async fn directoryExists(&self, path: &str) -> FilesystemResult<bool> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(disk) => disk.exists(path.into(), Some(true)).await,
            Driver::S3(disk) => disk.directory_exists(path).await,
        }
    }

    pub async fn delete(&self, path: &str) -> FilesystemResult<bool> {
        validate_path(path)?;
        let result = match &self.driver {
            Driver::Local(disk) => disk.delete(path.into()).await,
            Driver::S3(disk) => disk.delete(path).await,
        };
        self.or_default(result.map(|()| true))
    }

    pub async fn getVisibility(&self, path: &str) -> FilesystemResult<Visibility> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(disk) => disk.get_visibility(path.into()).await,
            Driver::S3(disk) => disk.get_visibility(path).await,
        }
    }

    pub async fn setVisibility(
        &self,
        path: &str,
        visibility: Visibility,
    ) -> FilesystemResult<bool> {
        validate_path(path)?;
        let result = match &self.driver {
            Driver::Local(disk) => disk.set_visibility(path.into(), visibility).await,
            Driver::S3(disk) => disk.set_visibility(path, visibility).await,
        };
        self.or_default(result.map(|()| true))
    }

    fn or_default<T: Default>(&self, result: FilesystemResult<T>) -> FilesystemResult<T> {
        match result {
            Err(error) if !self.throw => {
                if self.report {
                    let _ = crate::log::Log::error(&error.to_string(), &Default::default());
                }
                Ok(T::default())
            }
            result => result,
        }
    }
}

pub(super) fn validate_path(path: &str) -> FilesystemResult<()> {
    if path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| matches!(part, "" | "." | "..") || is_staging_name(part))
    {
        return Err(FilesystemError::InvalidPath);
    }
    Ok(())
}
