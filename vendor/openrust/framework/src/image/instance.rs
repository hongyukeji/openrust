use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use axum::body::Bytes;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    sync::OnceCell,
};

use crate::{
    filesystem::{FilesystemAdapter, Storage, Visibility, WriteOptions},
    http::UploadedFile,
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
    ConfiguredStorage(Option<String>, String),
    Url(String),
}

/// An owned, lazy image pipeline. Clone it to branch the transformations.
#[derive(Clone)]
pub struct Image {
    source: Source,
    file: Option<UploadedFile>,
    manager: ImageManager,
    pipeline: ImagePipeline,
    driver: Option<String>,
    rendered: Arc<OnceCell<EncodedImage>>,
    loaded: Arc<OnceCell<Bytes>>,
    hash_name: OnceLock<String>,
    dominant_color: OnceCell<String>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Image {
    pub(super) fn new(manager: ImageManager, source: Source) -> Self {
        let file = match &source {
            Source::Upload(file) => Some(file.clone()),
            _ => None,
        };
        Self {
            source,
            file,
            manager,
            pipeline: ImagePipeline::default(),
            driver: None,
            rendered: Arc::default(),
            loaded: Arc::default(),
            hash_name: OnceLock::new(),
            dominant_color: OnceCell::new(),
        }
    }

    pub fn file(&self) -> Option<&UploadedFile> {
        self.file.as_ref()
    }

    pub fn using(mut self, driver: impl Into<String>) -> Self {
        self.newClone();
        self.driver = Some(driver.into());
        self
    }

    pub fn transform(mut self, transformation: Transformation) -> Self {
        self.newClone();
        self.pipeline.transformations.push(transformation);
        self
    }

    pub fn resize(self, width: Option<u32>, height: Option<u32>) -> Result<Self> {
        if width.is_none() && height.is_none() {
            return Err(Error::InvalidDimensions);
        }
        Ok(self.transform(Transformation::Resize(
            width.map(|value| value.max(1)),
            height.map(|value| value.max(1)),
        )))
    }

    pub fn scale(self, width: Option<u32>, height: Option<u32>) -> Result<Self> {
        if width.is_none() && height.is_none() {
            return Err(Error::InvalidDimensions);
        }
        Ok(self.transform(Transformation::Scale(
            width.map(|value| value.max(1)),
            height.map(|value| value.max(1)),
        )))
    }

    pub fn cover(self, width: u32, height: u32) -> Self {
        self.transform(Transformation::Cover(width.max(1), height.max(1)))
    }

    pub fn contain(self, width: u32, height: u32, background: Background) -> Self {
        self.transform(Transformation::Contain(
            width.max(1),
            height.max(1),
            background,
        ))
    }

    pub fn crop(self, width: u32, height: u32, x: u32, y: u32) -> Self {
        self.transform(Transformation::Crop {
            width: width.max(1),
            height: height.max(1),
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
        self.transform(Transformation::Blur(amount.min(100)))
    }

    pub fn sharpen(self, amount: u8) -> Self {
        self.transform(Transformation::Sharpen(amount.min(100)))
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

    pub fn flip(self) -> Self {
        self.flipVertically()
    }

    pub fn flop(self) -> Self {
        self.flipHorizontally()
    }

    /// 选择应用已注册的 GD 驱动；未注册时在处理图片时返回 UnknownDriver。
    pub fn usingGd(self) -> Self {
        self.using("gd")
    }

    /// 选择应用已注册的 Imagick 驱动；未注册时在处理图片时返回 UnknownDriver。
    pub fn usingImagick(self) -> Self {
        self.using("imagick")
    }

    pub fn when(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
        if condition { transform(self) } else { self }
    }

    pub fn unless(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, transform)
    }

    pub fn encode(mut self, format: Format) -> Self {
        self.newClone();
        self.pipeline.format = Some(format);
        self
    }

    pub fn toFormat(self, format: &str) -> Result<Self> {
        Ok(self.encode(format.parse()?))
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
    pub fn toHeic(self) -> Self {
        self.encode(Format::Heic)
    }
    pub fn toJxl(self) -> Self {
        self.encode(Format::Jxl)
    }
    pub fn toBmp(self) -> Self {
        self.encode(Format::Bmp)
    }

    pub fn quality(mut self, quality: u8) -> Self {
        self.newClone();
        self.pipeline.quality = Some(quality.clamp(1, 100));
        self
    }

    pub fn optimize(self, format: Option<Format>, quality: Option<u8>) -> Self {
        self.encode(format.unwrap_or(Format::Webp))
            .quality(quality.unwrap_or(70))
    }

    pub async fn toBytes(&self) -> Result<Bytes> {
        Ok(self.render().await?.bytes.clone())
    }

    pub async fn to_base64(&self) -> Result<String> {
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

    pub async fn mime_type(&self) -> Result<&'static str> {
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
        self.dominant_color
            .get_or_try_init(|| async {
                let rendered = self.rendered.get();
                let contents = match rendered {
                    Some(rendered) => rendered.bytes.clone(),
                    None => self.load().await?,
                };
                let pipeline = if rendered.is_some() {
                    ImagePipeline::default()
                } else {
                    self.pipeline.clone()
                };
                let driver = self.manager.driver(self.driver.as_deref())?;
                let limits = self.manager.limits();
                self.manager
                    .blocking(move || {
                        let contents = if pipeline.transformations.is_empty()
                            && pipeline.format.is_none()
                            && pipeline.quality.is_none()
                        {
                            contents
                        } else {
                            driver.process(contents, &pipeline, limits)?.bytes
                        };
                        driver.dominantColor(contents, &ImagePipeline::default(), limits)
                    })
                    .await
            })
            .await
            .cloned()
    }

    pub async fn hash_name(&self, path: &str) -> Result<String> {
        let hash = self
            .hash_name
            .get_or_init(|| crate::support::Str::random(40));
        let name = format!("{hash}.{}", self.extension().await?);
        Ok(if path.is_empty() || path == "0" {
            name
        } else {
            format!("{path}/{name}")
        })
    }

    pub async fn to_string(&self) -> Result<String> {
        self.toDataUri().await
    }

    pub async fn to_response(&self, _: &mut crate::http::Request) -> Result<crate::http::Response> {
        let rendered = self.render().await?;
        let mut response =
            crate::http::Response::new(crate::http::StatusCode::OK, crate::http::Body::empty());
        response.set_content(rendered.bytes.clone());
        Ok(response.header(
            ::http::header::CONTENT_TYPE,
            ::http::HeaderValue::from_static(rendered.format.mime_type()),
        ))
    }

    fn newClone(&mut self) {
        if let Some(rendered) = self.rendered.get() {
            self.source = Source::Bytes(rendered.bytes.clone());
            self.loaded = Arc::default();
            self.pipeline = ImagePipeline::default();
        }
        self.rendered = Arc::default();
        self.dominant_color = OnceCell::new();
    }

    pub async fn store(&self, directory: &str, disk: Option<&str>) -> Result<Option<String>> {
        self.store_inner(directory, None, disk, None).await
    }

    pub async fn store_as<'a>(
        &self,
        directory: &str,
        name: impl Into<Option<&'a str>>,
        disk: Option<&str>,
    ) -> Result<Option<String>> {
        let (directory, name) = match name.into() {
            Some(name) => (directory, name),
            None => ("", directory),
        };
        self.store_inner(directory, Some(name), disk, None).await
    }

    pub async fn store_publicly(
        &self,
        directory: &str,
        disk: Option<&str>,
    ) -> Result<Option<String>> {
        self.store_inner(directory, None, disk, Some(Visibility::Public))
            .await
    }

    pub async fn store_publicly_as<'a>(
        &self,
        directory: &str,
        name: impl Into<Option<&'a str>>,
        disk: Option<&str>,
    ) -> Result<Option<String>> {
        let (directory, name) = match name.into() {
            Some(name) => (directory, name),
            None => ("", directory),
        };
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
            generated = self.hash_name("").await?;
            &generated
        };
        let path = format!("{directory}/{name}").trim_matches('/').to_owned();
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
                let driver = if self.pipeline.transformations.is_empty()
                    && self.pipeline.format.is_none()
                    && self.pipeline.quality.is_none()
                {
                    // 原样读取不解析指定后端，仍执行原生输入与尺寸限额检查。
                    Arc::new(super::native_driver::NativeDriver::default())
                        as Arc<dyn super::Driver>
                } else {
                    self.manager.driver(self.driver.as_deref())?
                };
                let pipeline = self.pipeline.clone();
                let limits = self.manager.limits();
                self.manager
                    .blocking(move || driver.process(contents, &pipeline, limits))
                    .await
            })
            .await
    }

    async fn load(&self) -> Result<Bytes> {
        self.loaded
            .get_or_try_init(|| self.read_source())
            .await
            .cloned()
    }

    async fn read_source(&self) -> Result<Bytes> {
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
            Source::Path(path) => read_bounded(tokio::fs::File::open(path).await?, limits).await?,
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
            Source::ConfiguredStorage(disk, path) => {
                let disk = self.manager.storage(disk.as_deref())?;
                read_bounded(
                    disk.readStream(path)
                        .await
                        .map_err(Error::Filesystem)?
                        .ok_or(Error::Missing)?,
                    limits,
                )
                .await?
            }
            Source::Url(url) => {
                let response = self
                    .manager
                    .http()?
                    .pending_request()
                    .max_response_bytes(limits.max_input_bytes)
                    .get(url)
                    .await
                    .map_err(Error::Http)?
                    .throw()
                    .map_err(Error::Http)?;
                if !response.successful() {
                    return Err(Error::Missing);
                }
                response.body().map_err(Error::Http)?
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
    reader.read_to_end(&mut buffer).await?;
    if buffer.len() > limits.max_input_bytes {
        return Err(Error::InputTooLarge);
    }
    Ok(buffer.into())
}

#[allow(non_snake_case)]
impl crate::contracts::support::Responsable for Image {
    async fn to_response(
        self,
        request: &mut crate::http::Request,
    ) -> std::result::Result<crate::http::Response, crate::http::HttpError> {
        Image::to_response(&self, request).await.map_err(Into::into)
    }
}
