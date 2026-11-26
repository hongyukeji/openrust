use std::{fmt, path::Path};

use bytes::Bytes;
use openrust::{http::UploadedFile, support::Str};
use serde::Deserialize;

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

#[derive(Default)]
pub(crate) struct AttachmentBudget {
    count: usize,
    bytes: usize,
}

impl AttachmentBudget {
    pub(crate) fn include(&mut self, attachments: &[Attachment]) -> AiResult<()> {
        self.count = self.count.saturating_add(attachments.len());
        if self.count > MAX_ATTACHMENTS {
            return Err(AiError::TooManyAttachments);
        }
        self.bytes = attachments
            .iter()
            .map(Attachment::len)
            .fold(self.bytes, usize::saturating_add);
        if self.bytes >= MAX_ATTACHMENT_BYTES {
            return Err(AiError::MediaTooLarge);
        }
        Ok(())
    }
}

pub(crate) fn validate_prompt_attachments(
    attachments: &[Attachment],
    messages: &[Message],
) -> AiResult<()> {
    let mut budget = AttachmentBudget::default();
    budget.include(attachments)?;
    for message in messages
        .iter()
        .filter(|message| !message.attachments.is_empty())
    {
        if message.role != MessageRole::User {
            return Err(AiError::AttachmentsNotSupported);
        }
        budget.include(&message.attachments)?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum StoredAttachment<'a> {
    #[serde(rename = "base64-image")]
    Image {
        base64: String,
        mime: std::borrow::Cow<'a, str>,
    },
    #[serde(rename = "base64-document")]
    Document {
        base64: String,
        mime: std::borrow::Cow<'a, str>,
        name: std::borrow::Cow<'a, str>,
    },
}

pub(crate) fn decode_stored_attachments(json: &str) -> AiResult<Vec<Attachment>> {
    const MAX_JSON_BYTES: usize = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 + MAX_ATTACHMENTS * 4096;
    if json.len() > MAX_JSON_BYTES {
        return Err(AiError::MediaTooLarge);
    }
    let stored: Vec<StoredAttachment<'_>> =
        serde_json::from_str(json).map_err(|_| AiError::InvalidResponse)?;
    restore_inline_attachments(stored)
}

pub(crate) fn restore_inline_attachments(
    stored: Vec<StoredAttachment<'_>>,
) -> AiResult<Vec<Attachment>> {
    if stored.len() > MAX_ATTACHMENTS {
        return Err(AiError::TooManyAttachments);
    }
    let mut attachments = Vec::with_capacity(stored.len());
    for attachment in stored {
        match attachment {
            StoredAttachment::Image { base64, mime } => {
                let bytes = Str::from_base64(&base64).map_err(|_| AiError::InvalidImage)?;
                let image = ImageAttachment::from_bytes(bytes)?;
                if image.mime() != mime {
                    return Err(AiError::InvalidImage);
                }
                attachments.push(image.into());
            }
            StoredAttachment::Document { base64, mime, name } => {
                let bytes = Str::from_base64(&base64).map_err(|_| AiError::InvalidDocument)?;
                let document = DocumentAttachment::from_bytes(name.into_owned(), bytes)?;
                if document.mime() != mime {
                    return Err(AiError::InvalidDocument);
                }
                attachments.push(document.into());
            }
        }
    }
    validate_prompt_attachments(&attachments, &[])?;
    Ok(attachments)
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
