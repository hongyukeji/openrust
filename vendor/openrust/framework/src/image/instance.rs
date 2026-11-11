use std::{path::PathBuf, sync::Arc};

use axum::body::Bytes;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    sync::OnceCell,
};

use crate::{
    filesystem::{FilesystemAdapter, Storage, Visibility, WriteOptions},
    http::{UploadedFile, client::Factory},
};

use super::{
    Background, EncodedImage, Error, Format, ImageManager, ImagePipeline, Limits, Result,
    Transformation,
};

#[derive(Clone)]
pub(super) enum Source {
    Bytes(Bytes),
    Base64(Arc<str>),
    Path(PathBuf),
    Storage(Arc<FilesystemAdapter>, String),
    Upload(UploadedFile),
    Url(Arc<Factory>, String),
}

/// An owned, lazy image pipeline. Clone it to branch the transformations.
#[derive(Clone)]
pub struct Image {
    source: Source,
    manager: ImageManager,
    pipeline: ImagePipeline,
    driver: Option<String>,
    rendered: Arc<OnceCell<EncodedImage>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Image API")]
impl Image {
    pub(super) fn new(manager: ImageManager, source: Source) -> Self {
        Self {
            source,
            manager,
            pipeline: ImagePipeline::default(),
            driver: None,
            rendered: Arc::default(),
        }
    }

    pub fn file(&self) -> Option<&UploadedFile> {
        match &self.source {
            Source::Upload(file) => Some(file),
            _ => None,
        }
    }

    pub fn using(mut self, driver: impl Into<String>) -> Self {
        self.driver = Some(driver.into());
        self.rendered = Arc::default();
        self
    }

    pub fn transform(mut self, transformation: Transformation) -> Self {
        self.pipeline.transformations.push(transformation);
        self.rendered = Arc::default();
        self
    }

    pub fn resize(self, width: Option<u32>, height: Option<u32>) -> Self {
        self.transform(Transformation::Resize(width, height))
    }

    pub fn scale(self, width: Option<u32>, height: Option<u32>) -> Self {
        self.transform(Transformation::Scale(width, height))
    }

    pub fn cover(self, width: u32, height: u32) -> Self {
        self.transform(Transformation::Cover(width, height))
    }

    pub fn contain(self, width: u32, height: u32, background: Background) -> Self {
        self.transform(Transformation::Contain(width, height, background))
    }

    pub fn crop(self, width: u32, height: u32, x: u32, y: u32) -> Self {
        self.transform(Transformation::Crop {
            width,
            height,
            x,
            y,
        })
    }

    pub fn orient(self) -> Self {
        self.transform(Transformation::Orient)
    }

    pub fn rotate(self, angle: f64, background: Background) -> Self {
        self.transform(Transformation::Rotate(angle, background))
    }

    pub fn blur(self, amount: u8) -> Self {
        self.transform(Transformation::Blur(amount))
    }

    pub fn sharpen(self, amount: u8) -> Self {
        self.transform(Transformation::Sharpen(amount))
    }

    pub fn grayscale(self) -> Self {
        self.transform(Transformation::Grayscale)
    }

    pub fn flipHorizontally(self) -> Self {
        self.transform(Transformation::FlipHorizontally)
    }

    pub fn flipVertically(self) -> Self {
        self.transform(Transformation::FlipVertically)
    }

    pub fn when(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
        if condition { transform(self) } else { self }
    }

    pub fn unless(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, transform)
    }

    pub fn encode(mut self, format: Format) -> Self {
        self.pipeline.format = Some(format);
        self.rendered = Arc::default();
        self
    }

    pub fn toJpeg(self) -> Self {
        self.encode(Format::Jpeg)
    }
    pub fn toJpg(self) -> Self {
        self.toJpeg()
    }
    pub fn toPng(self) -> Self {
        self.encode(Format::Png)
    }
    pub fn toGif(self) -> Self {
        self.encode(Format::Gif)
    }
    pub fn toWebp(self) -> Self {
        self.encode(Format::Webp)
    }
    pub fn toAvif(self) -> Self {
        self.encode(Format::Avif)
    }
    pub fn toBmp(self) -> Self {
        self.encode(Format::Bmp)
    }

    pub fn quality(mut self, quality: u8) -> Self {
        self.pipeline.quality = Some(quality.clamp(1, 100));
        self.rendered = Arc::default();
        self
    }

    pub fn optimize(self, format: Option<Format>, quality: Option<u8>) -> Self {
        self.encode(format.unwrap_or(Format::Webp))
            .quality(quality.unwrap_or(70))
    }

    pub async fn toBytes(&self) -> Result<Bytes> {
        Ok(self.render().await?.bytes.clone())
    }

    pub async fn toBase64(&self) -> Result<String> {
        Ok(STANDARD.encode(self.toBytes().await?))
    }

    pub async fn toDataUri(&self) -> Result<String> {
        let rendered = self.render().await?;
        Ok(format!(
            "data:{};base64,{}",
            rendered.format.mime_type(),
            STANDARD.encode(&rendered.bytes)
        ))
    }

    pub async fn mimeType(&self) -> Result<&'static str> {
        Ok(self.render().await?.format.mime_type())
    }

    pub async fn extension(&self) -> Result<&'static str> {
        Ok(self.render().await?.format.extension())
    }

    pub async fn dimensions(&self) -> Result<(u32, u32)> {
        Ok(self.render().await?.dimensions)
    }

    pub async fn width(&self) -> Result<u32> {
        Ok(self.dimensions().await?.0)
    }

    pub async fn height(&self) -> Result<u32> {
        Ok(self.dimensions().await?.1)
    }

    pub async fn dominantColor(&self) -> Result<String> {
        let contents = self.load().await?;
        let driver = self.manager.driver(self.driver.as_deref())?;
        let pipeline = self.pipeline.clone();
        let limits = self.manager.limits();
        self.manager
            .blocking(move || driver.dominantColor(contents, &pipeline, limits))
            .await
    }

    pub async fn store(&self, directory: &str, disk: Option<&str>) -> Result<Option<String>> {
        self.store_inner(directory, None, disk, None).await
    }

    pub async fn storeAs(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> Result<Option<String>> {
        self.store_inner(directory, Some(name), disk, None).await
    }

    pub async fn storePublicly(
        &self,
        directory: &str,
        disk: Option<&str>,
    ) -> Result<Option<String>> {
        self.store_inner(directory, None, disk, Some(Visibility::Public))
            .await
    }

    pub async fn storePubliclyAs(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> Result<Option<String>> {
        self.store_inner(directory, Some(name), disk, Some(Visibility::Public))
            .await
    }

    async fn store_inner(
        &self,
        directory: &str,
        name: Option<&str>,
        disk: Option<&str>,
        visibility: Option<Visibility>,
    ) -> Result<Option<String>> {
        let disk = Storage::disk(disk).map_err(Error::Filesystem)?;
        let rendered = self.render().await?;
        let generated;
        let name = if let Some(name) = name {
            name
        } else {
            generated = format!(
                "{}.{}",
                crate::http::random_upload_name().map_err(Error::Filesystem)?,
                rendered.format.extension()
            );
            &generated
        };
        let directory = directory.trim_matches('/');
        let path = if directory.is_empty() {
            name.to_owned()
        } else {
            format!("{directory}/{name}")
        };
        let written = disk
            .putWithOptions(&path, &rendered.bytes, WriteOptions { visibility })
            .await
            .map_err(Error::Filesystem)?;
        Ok(written.then_some(path))
    }

    async fn render(&self) -> Result<&EncodedImage> {
        self.rendered
            .get_or_try_init(|| async {
                let contents = self.load().await?;
                let driver = self.manager.driver(self.driver.as_deref())?;
                let pipeline = self.pipeline.clone();
                let limits = self.manager.limits();
                self.manager
                    .blocking(move || driver.process(contents, &pipeline, limits))
                    .await
            })
            .await
    }

    async fn load(&self) -> Result<Bytes> {
        let limits = self.manager.limits();
        let bytes = match &self.source {
            Source::Bytes(bytes) => bytes.clone(),
            Source::Upload(file) => file.get(),
            Source::Base64(encoded) => {
                if encoded.len() > limits.max_input_bytes.div_ceil(3).saturating_mul(4) {
                    return Err(Error::InputTooLarge);
                }
                STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(Error::Base64)?
                    .into()
            }
            Source::Path(path) => {
                read_bounded(
                    tokio::fs::File::open(path).await.map_err(Error::Io)?,
                    limits,
                )
                .await?
            }
            Source::Storage(disk, path) => {
                read_bounded(
                    disk.readStream(path)
                        .await
                        .map_err(Error::Filesystem)?
                        .ok_or(Error::Missing)?,
                    limits,
                )
                .await?
            }
            Source::Url(factory, url) => {
                let response = factory
                    .pendingRequest()
                    .maxResponseBytes(limits.max_input_bytes)
                    .get(url)
                    .await
                    .map_err(Error::Http)?
                    .throw()
                    .map_err(Error::Http)?;
                if !response.successful() {
                    return Err(Error::Missing);
                }
                Bytes::copy_from_slice(response.body())
            }
        };
        if bytes.len() > limits.max_input_bytes {
            return Err(Error::InputTooLarge);
        }
        Ok(bytes)
    }
}

pub(super) async fn read_bounded(reader: impl AsyncRead, limits: Limits) -> Result<Bytes> {
    let mut reader = Box::pin(reader.take((limits.max_input_bytes as u64).saturating_add(1)));
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).await.map_err(Error::Io)?;
    if buffer.len() > limits.max_input_bytes {
        return Err(Error::InputTooLarge);
    }
    Ok(buffer.into())
}
