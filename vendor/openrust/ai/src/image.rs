use std::{sync::Arc, time::Duration};

use bytes::Bytes;
pub use openrust::image::Format as ImageFormat;
use openrust::{
    filesystem::Visibility,
    http::{HttpFuture, UploadedFile, client::Part},
    support::Str,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Ai, AiError, AiResult, Usage,
    connection::Connection,
    media_storage,
    text::{transport_error, validate},
};

const MAX_ATTACHMENTS: usize = 16;
const MAX_ATTACHMENT_BYTES: usize = 50_000_000;

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

    pub async fn generate(self) -> AiResult<ImageResponse> {
        self.generate_with(None, None).await
    }

    pub async fn generate_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<ImageResponse> {
        let provider = Ai::image_provider(provider)?;
        let model = match model {
            Some(model) => model,
            None => provider.default_image_model()?,
        };
        let request = ImagePrompt {
            prompt: &self.prompt,
            attachments: &self.attachments,
            model,
            size: self.size.as_deref(),
            quality: self.quality.as_deref(),
            timeout: self.timeout,
        };
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
}

impl ImageAttachment {
    pub fn from_bytes(content: impl Into<Bytes>) -> AiResult<Self> {
        let content = content.into();
        if content.len() >= MAX_ATTACHMENT_BYTES {
            return Err(AiError::InvalidImage);
        }
        let format = detect_format(&content).ok_or(AiError::InvalidImage)?;
        Ok(Self { content, format })
    }

    pub async fn from_storage(path: &str, disk: Option<&str>) -> AiResult<Self> {
        let content = media_storage::read(path, disk, MAX_ATTACHMENT_BYTES)
            .await
            .map_err(|error| {
                if matches!(error, AiError::MediaTooLarge) {
                    AiError::InvalidImage
                } else {
                    error
                }
            })?
            .ok_or(AiError::InvalidImage)?;
        Self::from_bytes(content)
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
        format!(
            "data:{};base64,{}",
            self.mime(),
            Str::to_base64(&self.content)
        )
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
                    },
                    image_input_tokens: usage["input_tokens_details"]["image_tokens"].as_u64(),
                    image_output_tokens: usage["output_tokens_details"]["image_tokens"].as_u64(),
                },
            })
        })
    }
}
