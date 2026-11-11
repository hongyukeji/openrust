use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt as _};

use super::{
    FilesystemError, FilesystemResult, LocalFilesystemAdapter, Visibility, WriteOptions,
    is_staging_name,
};

/// Applies disk configuration and exposes filesystem operations.
pub struct FilesystemAdapter {
    local: LocalFilesystemAdapter,
    throw: bool,
    report: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's filesystem adapter API")]
impl FilesystemAdapter {
    pub(super) fn local(local: LocalFilesystemAdapter, throw: bool, report: bool) -> Self {
        Self {
            local,
            throw,
            report,
        }
    }

    pub fn path(&self, path: &str) -> FilesystemResult<PathBuf> {
        Ok(self.local.root().join(validated_path(path)?))
    }

    pub async fn get(&self, path: &str) -> FilesystemResult<Option<Vec<u8>>> {
        let path = validated_path(path)?;
        let result = async {
            let mut file = self.local.read(path).await?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).await?;
            Ok(bytes)
        }
        .await;
        self.or_default(result.map(Some))
    }

    pub async fn readStream(&self, path: &str) -> FilesystemResult<Option<tokio::fs::File>> {
        let path = validated_path(path)?;
        self.or_default(self.local.read(path).await.map(Some))
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
        let path = validated_path(path)?;
        self.or_default(
            self.local
                .write(path, reader, options.visibility)
                .await
                .map(|()| true),
        )
    }

    pub async fn exists(&self, path: &str) -> FilesystemResult<bool> {
        self.local.exists(validated_path(path)?, None).await
    }

    pub async fn missing(&self, path: &str) -> FilesystemResult<bool> {
        Ok(!self.exists(path).await?)
    }

    pub async fn fileExists(&self, path: &str) -> FilesystemResult<bool> {
        self.local.exists(validated_path(path)?, Some(false)).await
    }

    pub async fn directoryExists(&self, path: &str) -> FilesystemResult<bool> {
        self.local.exists(validated_path(path)?, Some(true)).await
    }

    pub async fn delete(&self, path: &str) -> FilesystemResult<bool> {
        let path = validated_path(path)?;
        self.or_default(self.local.delete(path).await.map(|()| true))
    }

    pub async fn getVisibility(&self, path: &str) -> FilesystemResult<Visibility> {
        self.local.get_visibility(validated_path(path)?).await
    }

    pub async fn setVisibility(
        &self,
        path: &str,
        visibility: Visibility,
    ) -> FilesystemResult<bool> {
        let path = validated_path(path)?;
        self.or_default(
            self.local
                .set_visibility(path, visibility)
                .await
                .map(|()| true),
        )
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

fn validated_path(path: &str) -> FilesystemResult<PathBuf> {
    if path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| matches!(part, "" | "." | "..") || is_staging_name(part))
    {
        return Err(FilesystemError::InvalidPath);
    }
    Ok(Path::new(path).to_path_buf())
}
