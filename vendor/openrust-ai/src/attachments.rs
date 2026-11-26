use std::{fmt, path::Path};

use bytes::Bytes;
use openrust::{http::UploadedFile, support::Str};

use crate::{AiError, AiResult, ImageAttachment, Message, MessageRole, media_storage};

pub(crate) const MAX_ATTACHMENTS: usize = 16;
pub(crate) const MAX_ATTACHMENT_BYTES: usize = 50_000_000;

/// Agent 本轮调用使用的本地附件。
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Attachment {
    Image(ImageAttachment),
    Document(DocumentAttachment),
}

impl From<ImageAttachment> for Attachment {
    fn from(image: ImageAttachment) -> Self {
        Self::Image(image)
    }
}

impl From<DocumentAttachment> for Attachment {
    fn from(document: DocumentAttachment) -> Self {
        Self::Document(document)
    }
}

impl Attachment {
    fn len(&self) -> usize {
        match self {
            Self::Image(image) => image.content().len(),
            Self::Document(document) => document.content().len(),
        }
    }
}

pub(crate) fn validate_prompt_attachments(
    attachments: &[Attachment],
    messages: &[Message],
) -> AiResult<()> {
    let mut count = attachments.len();
    let mut bytes = attachments
        .iter()
        .map(Attachment::len)
        .fold(0usize, usize::saturating_add);
    for message in messages
        .iter()
        .filter(|message| !message.attachments.is_empty())
    {
        if message.role != MessageRole::User {
            return Err(AiError::AttachmentsNotSupported);
        }
        count = count.saturating_add(message.attachments.len());
        bytes = message
            .attachments
            .iter()
            .map(Attachment::len)
            .fold(bytes, usize::saturating_add);
    }
    if count > MAX_ATTACHMENTS {
        return Err(AiError::TooManyAttachments);
    }
    if bytes >= MAX_ATTACHMENT_BYTES {
        return Err(AiError::MediaTooLarge);
    }
    Ok(())
}

pub(crate) fn data_url(mime: &str, content: &[u8]) -> String {
    format!("data:{mime};base64,{}", Str::to_base64(content))
}

/// 已校验文件名及格式的内联文档。
#[derive(Clone, PartialEq, Eq)]
pub struct DocumentAttachment {
    filename: String,
    content: Bytes,
    mime: &'static str,
}

impl fmt::Debug for DocumentAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentAttachment")
            .field("mime", &self.mime)
            .field("bytes", &self.content.len())
            .finish()
    }
}

impl DocumentAttachment {
    pub fn from_bytes(filename: impl Into<String>, content: impl Into<Bytes>) -> AiResult<Self> {
        let filename = filename.into();
        let content = content.into();
        if content.len() >= MAX_ATTACHMENT_BYTES {
            return Err(AiError::MediaTooLarge);
        }
        if filename.trim().is_empty()
            || filename.len() > 255
            || filename.contains(['/', '\\'])
            || filename.chars().any(char::is_control)
        {
            return Err(AiError::InvalidDocument);
        }
        let extension = Path::new(&filename)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let mime = if extension.eq_ignore_ascii_case("pdf") && content.starts_with(b"%PDF-") {
            "application/pdf"
        } else if !content.is_empty()
            && !content.contains(&0)
            && std::str::from_utf8(&content).is_ok()
        {
            match extension.to_ascii_lowercase().as_str() {
                "txt" => "text/plain",
                "md" => "text/markdown",
                _ => return Err(AiError::InvalidDocument),
            }
        } else {
            return Err(AiError::InvalidDocument);
        };
        Ok(Self {
            filename,
            content,
            mime,
        })
    }

    pub fn from_upload(file: &UploadedFile) -> AiResult<Self> {
        Self::from_bytes(file.get_client_original_name(), file.get())
    }

    pub async fn from_path(path: impl AsRef<Path>) -> AiResult<Self> {
        let path = path.as_ref();
        Self::from_bytes(
            filename(path)?,
            media_storage::read_path(path, MAX_ATTACHMENT_BYTES).await?,
        )
    }

    pub async fn from_storage(path: &str, disk: Option<&str>) -> AiResult<Self> {
        let name = filename(Path::new(path))?;
        let content = media_storage::read(path, disk, MAX_ATTACHMENT_BYTES)
            .await?
            .ok_or(AiError::InvalidDocument)?;
        Self::from_bytes(name, content)
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn mime(&self) -> &'static str {
        self.mime
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub(crate) fn data_url(&self) -> String {
        data_url(self.mime, &self.content)
    }

    pub(crate) fn is_pdf(&self) -> bool {
        self.mime == "application/pdf"
    }

    pub(crate) fn text(&self) -> AiResult<&str> {
        std::str::from_utf8(&self.content).map_err(|_| AiError::InvalidDocument)
    }
}

fn filename(path: &Path) -> AiResult<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or(AiError::InvalidDocument)
}
