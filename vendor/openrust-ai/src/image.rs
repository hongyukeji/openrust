use std::{fmt, path::Path, sync::Arc, time::Duration};

use bytes::Bytes;
pub use openrust::image::Format as ImageFormat;
use openrust::{
    filesystem::{Storage, Visibility},
    http::{HttpFuture, UploadedFile, client::Part},
    support::{Str, facades::Facade},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Ai, AiError, AiResult, FakeImageGateway, MediaQueueCatch, MediaQueueThen, QueuedImageResponse,
    QueuedMediaSource, RecordedImage, RecordedQueuedImage, Usage,
    attachments::{MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, data_url},
    connection::Connection,
    media_storage,
    text::{transport_error, validate},
};

pub struct Image;

impl Image {
    pub fn of(prompt: impl Into<String>) -> AiResult<PendingImageGeneration> {
        let prompt = prompt.into();
        if prompt.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        Ok(PendingImageGeneration {
            prompt,
            attachments: Vec::new(),
            size: None,
            quality: None,
            timeout: None,
        })
    }

    pub fn register_queue_then(
        name: impl Into<String>,
        callback: MediaQueueThen<ImageResponse>,
    ) -> openrust::queue::QueueResult<()> {
        Ai::manager()
            .map_err(openrust::queue::QueueError::execution)?
            .image_queue_callbacks()
            .register_then(name, callback)
    }

    pub fn register_queue_catch(
        name: impl Into<String>,
        callback: MediaQueueCatch,
    ) -> openrust::queue::QueueResult<()> {
        Ai::manager()
            .map_err(openrust::queue::QueueError::execution)?
            .image_queue_callbacks()
            .register_catch(name, callback)
    }

    pub fn fake(
        responses: impl IntoIterator<Item = ImageResponse>,
    ) -> AiResult<Arc<FakeImageGateway>> {
        Ai::fake_images(responses)
    }

    pub fn fake_with(
        resolve: impl Fn(&RecordedImage) -> AiResult<ImageResponse> + Send + Sync + 'static,
    ) -> AiResult<Arc<FakeImageGateway>> {
        Ai::fake_images_with(resolve)
    }

    pub fn is_faked() -> AiResult<bool> {
        Ai::images_are_faked()
    }

    fn fake_gateway() -> AiResult<Arc<FakeImageGateway>> {
        Ai::manager()?.image_fake().ok_or(AiError::FakeNotInstalled)
    }

    #[track_caller]
    pub fn assert_queued(predicate: impl Fn(&RecordedQueuedImage) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_queued(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_queued(predicate: impl Fn(&RecordedQueuedImage) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_queued(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_queued() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_queued();
        Ok(())
    }

    #[track_caller]
    pub fn assert_generated(predicate: impl Fn(&RecordedImage) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_generated(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_generated(predicate: impl Fn(&RecordedImage) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_generated(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_generated() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_generated();
        Ok(())
    }
}

pub struct PendingImageGeneration {
    prompt: String,
    attachments: Vec<ImageAttachment>,
    size: Option<String>,
    quality: Option<String>,
    timeout: Option<Duration>,
}

impl PendingImageGeneration {
    pub fn attachments(mut self, images: impl IntoIterator<Item = ImageAttachment>) -> Self {
        self.attachments = images.into_iter().collect();
        self
    }

    pub fn size(mut self, size: impl Into<String>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn square(self) -> Self {
        self.size("1:1")
    }

    pub fn portrait(self) -> Self {
        self.size("2:3")
    }

    pub fn landscape(self) -> Self {
        self.size("3:2")
    }

    pub fn quality(mut self, quality: impl Into<String>) -> Self {
        self.quality = Some(quality.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn queue(self) -> AiResult<QueuedImageResponse> {
        self.queue_with(None, None)
    }

    pub fn queue_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<QueuedImageResponse> {
        self.request(model.unwrap_or("queued")).validate()?;
        let attachments = self
            .attachments
            .into_iter()
            .map(|attachment| {
                attachment
                    .source
                    .ok_or(AiError::InvalidConfiguration("images.queue_source"))
            })
            .collect::<AiResult<_>>()?;
        Ok(QueuedImageResponse::image(RecordedQueuedImage {
            prompt: self.prompt,
            attachments,
            size: self.size,
            quality: self.quality,
            timeout: self.timeout,
            provider: provider.map(str::to_owned),
            model: model.map(str::to_owned),
        }))
    }

    pub(crate) async fn from_queued(prompt: &RecordedQueuedImage) -> AiResult<Self> {
        if prompt.attachments.len() > MAX_ATTACHMENTS {
            return Err(AiError::InvalidImage);
        }
        let mut pending = Image::of(&prompt.prompt)?;
        for attachment in &prompt.attachments {
            let image = match attachment {
                QueuedMediaSource::Path { path } if path.is_absolute() => {
                    ImageAttachment::from_path(path).await?
                }
                QueuedMediaSource::Path { .. } => return Err(AiError::InvalidImage),
                QueuedMediaSource::Storage { disk, path } => {
                    ImageAttachment::from_storage(path, Some(disk)).await?
                }
            };
            pending.attachments.push(image);
        }
        pending.size.clone_from(&prompt.size);
        pending.quality.clone_from(&prompt.quality);
        pending.timeout = prompt.timeout;
        Ok(pending)
    }

    fn request<'a>(&'a self, model: &'a str) -> ImagePrompt<'a> {
        ImagePrompt {
            prompt: &self.prompt,
            attachments: &self.attachments,
            model,
            size: self.size.as_deref(),
            quality: self.quality.as_deref(),
            timeout: self.timeout,
        }
    }

    pub async fn generate(self) -> AiResult<ImageResponse> {
        self.generate_with(None, None).await
    }

    pub async fn generate_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<ImageResponse> {
        let manager = Ai::manager()?;
        if let Some(fake) = manager.image_fake() {
            let selected_model = model.map(str::to_owned).or_else(|| {
                manager
                    .image_provider(provider)
                    .ok()?
                    .default_image_model()
                    .ok()
                    .map(str::to_owned)
            });
            self.request(selected_model.as_deref().unwrap_or("fake"))
                .validate()?;
            return fake.respond(RecordedImage {
                prompt: self.prompt,
                attachments: self.attachments,
                size: self.size,
                quality: self.quality,
                timeout: self.timeout,
                provider: manager
                    .provider_name_for(provider, "default_for_images")
                    .ok(),
                model: selected_model,
            });
        }
        let provider = manager.image_provider(provider)?;
        let model = match model {
            Some(model) => model,
            None => provider.default_image_model()?,
        };
        let request = self.request(model);
        request.validate()?;
        provider.generate_image(request).await
    }
}

pub struct ImagePrompt<'a> {
    pub prompt: &'a str,
    pub attachments: &'a [ImageAttachment],
    pub model: &'a str,
    pub size: Option<&'a str>,
    pub quality: Option<&'a str>,
    pub timeout: Option<Duration>,
}

impl ImagePrompt<'_> {
    fn validate(&self) -> AiResult<()> {
        if self.prompt.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        if self.model.trim().is_empty() {
            return Err(AiError::MissingModel);
        }
        if self.attachments.len() > MAX_ATTACHMENTS {
            return Err(AiError::InvalidImage);
        }
        if self.size.is_some_and(|value| value.trim().is_empty())
            || self.quality.is_some_and(|value| value.trim().is_empty())
            || self.timeout.is_some_and(|value| value.is_zero())
        {
            return Err(AiError::InvalidConfiguration("images.generation"));
        }
        Ok(())
    }
}

pub trait ImageProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_image_model(&self) -> AiResult<&str>;
    fn generate_image<'a>(
        &'a self,
        prompt: ImagePrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ImageResponse>>;
}

fn detect_format(bytes: &Bytes) -> Option<ImageFormat> {
    let uploaded = UploadedFile::from_bytes(String::new(), None, bytes.clone());
    match uploaded.extension() {
        Some("png") => Some(ImageFormat::Png),
        Some("jpg" | "jpeg") => Some(ImageFormat::Jpeg),
        Some("webp") => Some(ImageFormat::Webp),
        _ => None,
    }
}

#[derive(Clone)]
pub struct ImageAttachment {
    content: Bytes,
    format: ImageFormat,
    source: Option<QueuedMediaSource>,
}

impl PartialEq for ImageAttachment {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content && self.format == other.format
    }
}

impl Eq for ImageAttachment {}

impl fmt::Debug for ImageAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageAttachment")
            .field("format", &self.format)
            .field("bytes", &self.content.len())
            .finish()
    }
}

impl ImageAttachment {
    pub fn from_bytes(content: impl Into<Bytes>) -> AiResult<Self> {
        let content = content.into();
        if content.len() >= MAX_ATTACHMENT_BYTES {
            return Err(AiError::InvalidImage);
        }
        let format = detect_format(&content).ok_or(AiError::InvalidImage)?;
        Ok(Self {
            content,
            format,
            source: None,
        })
    }

    pub fn from_upload(file: &UploadedFile) -> AiResult<Self> {
        Self::from_bytes(file.get())
    }

    pub async fn from_path(path: impl AsRef<Path>) -> AiResult<Self> {
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(|_| AiError::MediaStorageUnavailable)?;
        let content = media_storage::read_path(&path, MAX_ATTACHMENT_BYTES)
            .await
            .map_err(|error| {
                if matches!(error, AiError::MediaTooLarge) {
                    AiError::InvalidImage
                } else {
                    error
                }
            })?;
        let mut image = Self::from_bytes(content)?;
        image.source = Some(QueuedMediaSource::Path { path });
        Ok(image)
    }

    pub async fn from_storage(path: &str, disk: Option<&str>) -> AiResult<Self> {
        let disk = match disk.filter(|name| !name.is_empty()) {
            Some(disk) => disk.to_owned(),
            None => Storage::get_facade_root()
                .map_err(|_| AiError::MediaStorageUnavailable)?
                .get_default_driver()
                .map_err(|_| AiError::MediaStorageUnavailable)?,
        };
        let content = media_storage::read(path, Some(&disk), MAX_ATTACHMENT_BYTES)
            .await
            .map_err(|error| {
                if matches!(error, AiError::MediaTooLarge) {
                    AiError::InvalidImage
                } else {
                    error
                }
            })?
            .ok_or(AiError::InvalidImage)?;
        let mut image = Self::from_bytes(content)?;
        image.source = Some(QueuedMediaSource::Storage {
            disk,
            path: path.to_owned(),
        });
        Ok(image)
    }

    pub fn mime(&self) -> &'static str {
        self.format.mime_type()
    }

    pub(crate) fn data_url(&self) -> String {
        data_url(self.mime(), &self.content)
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ImageUsage {
    #[serde(flatten)]
    pub tokens: Usage,
    pub image_input_tokens: Option<u64>,
    pub image_output_tokens: Option<u64>,
}

pub struct GeneratedImage {
    content: Bytes,
    format: ImageFormat,
    pub revised_prompt: Option<String>,
}

impl GeneratedImage {
    pub fn from_bytes(content: impl Into<Bytes>) -> AiResult<Self> {
        let content = content.into();
        let format = detect_format(&content).ok_or(AiError::InvalidImage)?;
        Ok(Self {
            content,
            format,
            revised_prompt: None,
        })
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub fn mime(&self) -> &'static str {
        self.format.mime_type()
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn data_url(&self) -> String {
        data_url(self.mime(), &self.content)
    }

    pub async fn store(&self, directory: &str, disk: Option<&str>) -> AiResult<String> {
        self.write(directory, None, disk, None).await
    }

    pub async fn store_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        self.write(directory, Some(name), disk, None).await
    }

    pub async fn store_publicly(&self, directory: &str, disk: Option<&str>) -> AiResult<String> {
        self.write(directory, None, disk, Some(Visibility::Public))
            .await
    }

    pub async fn store_publicly_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        self.write(directory, Some(name), disk, Some(Visibility::Public))
            .await
    }

    async fn write(
        &self,
        directory: &str,
        name: Option<&str>,
        disk: Option<&str>,
        visibility: Option<Visibility>,
    ) -> AiResult<String> {
        media_storage::write(
            &self.content,
            self.format.extension(),
            directory,
            name,
            disk,
            visibility,
        )
        .await
    }
}

pub struct ImageResponse {
    pub images: Vec<GeneratedImage>,
    pub usage: ImageUsage,
    pub provider: String,
    pub model: String,
}

impl ImageResponse {
    pub(crate) fn fake_placeholder(provider: String, model: String) -> Self {
        Self {
            images: vec![GeneratedImage {
                content: Bytes::from_static(b"fake-image-content"),
                format: ImageFormat::Png,
                revised_prompt: None,
            }],
            usage: ImageUsage::default(),
            provider,
            model,
        }
    }

    pub fn first_image(&self) -> AiResult<&GeneratedImage> {
        self.images.first().ok_or(AiError::InvalidResponse)
    }

    pub async fn store(&self, directory: &str, disk: Option<&str>) -> AiResult<String> {
        self.first_image()?.store(directory, disk).await
    }

    pub async fn store_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        self.first_image()?.store_as(directory, name, disk).await
    }

    pub async fn store_publicly(&self, directory: &str, disk: Option<&str>) -> AiResult<String> {
        self.first_image()?.store_publicly(directory, disk).await
    }

    pub async fn store_publicly_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        self.first_image()?
            .store_publicly_as(directory, name, disk)
            .await
    }
}

pub(crate) struct OpenAiImageProvider(pub(crate) Arc<Connection>);

impl ImageProvider for OpenAiImageProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_image_model(&self) -> AiResult<&str> {
        self.0.default_image_model()
    }

    fn generate_image<'a>(
        &'a self,
        prompt: ImagePrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ImageResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            let mut body = json!({"model":prompt.model,"prompt":prompt.prompt});
            if let Some(size) = prompt.size {
                body["size"] = match size {
                    "1:1" => "1024x1024",
                    "2:3" => "1024x1536",
                    "3:2" => "1536x1024",
                    size => size,
                }
                .into();
            }
            if let Some(quality) = prompt.quality {
                body["quality"] = quality.into();
            }
            let mut request = self.0.request(prompt.timeout)?;
            let editing = !prompt.attachments.is_empty();
            let endpoint = if editing {
                "images/edits"
            } else {
                "images/generations"
            };
            if editing {
                let name = if prompt.attachments.len() == 1 {
                    "image"
                } else {
                    "image[]"
                };
                for (position, reference) in prompt.attachments.iter().enumerate() {
                    let part = Part::stream_with_length(
                        reference.content.clone(),
                        reference.content.len() as u64,
                    )
                    .file_name(format!(
                        "reference-{position}.{}",
                        reference.format.extension()
                    ))
                    .mime_str(reference.format.mime_type())
                    .map_err(|_| AiError::InvalidImage)?;
                    request = request.attach(name, part);
                }
            }
            let response = request
                .post(endpoint, &body)
                .await
                .map_err(transport_error)?;
            let data: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let images = data["data"]
                .as_array()
                .filter(|images| !images.is_empty())
                .ok_or(AiError::InvalidResponse)?;
            let images = images
                .iter()
                .map(|encoded| {
                    let raw = encoded["b64_json"]
                        .as_str()
                        .ok_or(AiError::InvalidResponse)?;
                    let bytes = Str::from_base64(raw).map_err(|_| AiError::InvalidResponse)?;
                    let mut image =
                        GeneratedImage::from_bytes(bytes).map_err(|_| AiError::InvalidResponse)?;
                    image.revised_prompt = encoded["revised_prompt"].as_str().map(str::to_owned);
                    Ok(image)
                })
                .collect::<AiResult<Vec<_>>>()?;
            let usage = &data["usage"];
            Ok(ImageResponse {
                images,
                provider: self.name().into(),
                model: data["model"].as_str().unwrap_or(prompt.model).into(),
                usage: ImageUsage {
                    tokens: Usage {
                        prompt_tokens: usage["input_tokens"].as_u64(),
                        completion_tokens: usage["output_tokens"].as_u64(),
                        ..Usage::default()
                    },
                    image_input_tokens: usage["input_tokens_details"]["image_tokens"].as_u64(),
                    image_output_tokens: usage["output_tokens_details"]["image_tokens"].as_u64(),
                },
            })
        })
    }
}
