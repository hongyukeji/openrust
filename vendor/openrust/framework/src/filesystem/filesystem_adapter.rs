use std::sync::Arc;
use std::{path::PathBuf, time::SystemTime};

use tokio::io::{AsyncRead, AsyncReadExt as _};

use crate::http::{HeaderMap, Response, UploadedFile};

use super::{
    AwsS3V3Adapter, FilesystemError, FilesystemReader, FilesystemResult, LocalFilesystemAdapter,
    TemporaryUploadUrl, Visibility, WriteOptions, download::OpenedFile, is_staging_name,
};

/// Applies disk configuration and exposes filesystem operations.
pub struct FilesystemAdapter {
    driver: Driver,
    url: Option<String>,
    signed_route: Option<String>,
    throw: bool,
    report: bool,
}

enum Driver {
    Local(LocalFilesystemAdapter),
    S3(Box<AwsS3V3Adapter>),
}

#[allow(non_snake_case, reason = "Matches Laravel's filesystem adapter API")]
impl FilesystemAdapter {
    pub(super) fn local(
        local: LocalFilesystemAdapter,
        throw: bool,
        report: bool,
        url: Option<String>,
    ) -> Self {
        Self {
            driver: Driver::Local(local),
            url,
            signed_route: None,
            throw,
            report,
        }
    }

    pub(super) fn s3(
        driver: AwsS3V3Adapter,
        throw: bool,
        report: bool,
        url: Option<String>,
    ) -> Self {
        Self {
            driver: Driver::S3(Box::new(driver)),
            url,
            signed_route: None,
            throw,
            report,
        }
    }

    pub(super) fn serve_as(&mut self, name: &str) {
        if matches!(self.driver, Driver::Local(_)) {
            self.signed_route = Some(format!("storage.{name}"));
        }
    }

    pub fn providesTemporaryUrls(&self) -> bool {
        self.signed_route.is_some() || matches!(self.driver, Driver::S3(_))
    }

    pub async fn temporaryUrl(
        &self,
        path: &str,
        expiration: SystemTime,
    ) -> FilesystemResult<String> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(_) => self.temporary_local_url(path, expiration, false),
            Driver::S3(disk) => disk.temporary_url(path, expiration).await,
        }
    }

    pub async fn temporaryUploadUrl(
        &self,
        path: &str,
        expiration: SystemTime,
    ) -> FilesystemResult<TemporaryUploadUrl> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(_) => Ok(TemporaryUploadUrl {
                url: self.temporary_local_url(path, expiration, true)?,
                headers: HeaderMap::new(),
            }),
            Driver::S3(disk) => disk.temporary_upload_url(path, expiration).await,
        }
    }

    fn temporary_local_url(
        &self,
        path: &str,
        expiration: SystemTime,
        upload: bool,
    ) -> FilesystemResult<String> {
        use crate::{
            contracts::routing::UrlGenerator as _,
            routing::{RouteParameter, SignatureMode},
            support::facades::URL,
        };
        let name = self
            .signed_route
            .as_deref()
            .ok_or(FilesystemError::UnsupportedOperation)?;
        let name = if upload {
            format!("{name}.upload")
        } else {
            name.to_owned()
        };
        let mut parameters = vec![RouteParameter::named(
            "path",
            path.replace('%', "%25")
                .replace('?', "%3F")
                .replace('#', "%23")
                .replace('&', "%26"),
        )];
        if upload {
            parameters.push(RouteParameter::named("upload", "1"));
        }
        let url = (|| {
            let urls = URL::generator()?;
            let relative =
                urls.temporarySignedRoute(&name, expiration, &parameters, SignatureMode::Relative)?;
            urls.to(&relative, &[], None)
        })()
        .map_err(
            |source: crate::routing::UrlError| FilesystemError::Backend {
                source: Box::new(source),
            },
        )?;
        Ok(url)
    }

    pub fn path(&self, path: &str) -> FilesystemResult<PathBuf> {
        validate_path(path)?;
        Ok(match &self.driver {
            Driver::Local(disk) => disk.root().join(path),
            Driver::S3(disk) => PathBuf::from(disk.key(path)),
        })
    }

    pub fn image(
        self: &Arc<Self>,
        path: impl Into<String>,
    ) -> crate::image::Result<crate::image::Image> {
        crate::image::ImageFacade::fromDisk(self.clone(), path)
    }

    pub async fn get(&self, path: &str) -> FilesystemResult<Option<Vec<u8>>> {
        let Some(mut reader) = self.readStream(path).await? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        let result = reader.read_to_end(&mut bytes).await.map(|_| Some(bytes));
        self.or_default(result.map_err(FilesystemError::from))
    }

    /// Builds an object URL using the disk configuration.
    pub fn url(&self, path: &str) -> FilesystemResult<String> {
        validate_path(path)?;
        match &self.driver {
            Driver::Local(_) => Ok(match self.url.as_deref() {
                Some(base) => format!("{}/{path}", base.trim_end_matches('/')),
                None => format!("/storage/{}", path.strip_prefix("public/").unwrap_or(path)),
            }),
            Driver::S3(disk) => disk.url(path, self.url.as_deref()),
        }
    }

    /// Streams an attachment. The caller authorizes access before invoking this method.
    pub async fn download(
        &self,
        path: &str,
        name: Option<&str>,
        headers: HeaderMap,
    ) -> FilesystemResult<Response> {
        self.file_response(path, name, headers, "attachment").await
    }

    /// Streams a file with an inline disposition.
    pub async fn response(
        &self,
        path: &str,
        name: Option<&str>,
        headers: HeaderMap,
    ) -> FilesystemResult<Response> {
        self.file_response(path, name, headers, "inline").await
    }

    async fn file_response(
        &self,
        path: &str,
        name: Option<&str>,
        headers: HeaderMap,
        disposition: &str,
    ) -> FilesystemResult<Response> {
        validate_path(path)?;
        let file = match &self.driver {
            Driver::Local(disk) => {
                let reader = disk.read(path.into()).await?;
                let length = reader.metadata().await?.len();
                OpenedFile {
                    length: Some(length),
                    content_type: None,
                    reader: Box::new(reader.take(length)),
                }
            }
            Driver::S3(disk) => disk.open(path).await?,
        };
        file.response(path, name, headers, disposition)
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

    pub async fn putFile(
        &self,
        directory: &str,
        file: &UploadedFile,
        options: WriteOptions,
    ) -> FilesystemResult<Option<String>> {
        self.putFileAs(directory, file, file.hashName()?, options)
            .await
    }

    pub async fn putFileAs(
        &self,
        directory: &str,
        file: &UploadedFile,
        name: &str,
        options: WriteOptions,
    ) -> FilesystemResult<Option<String>> {
        let directory = directory.trim_matches('/');
        let path = if directory.is_empty() {
            name.to_owned()
        } else {
            format!("{directory}/{name}")
        };
        let stored = self.putWithOptions(&path, file.get(), options).await?;
        Ok(stored.then_some(path))
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
