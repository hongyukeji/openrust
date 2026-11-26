use openrust::{
    filesystem::{Storage, Visibility, WriteOptions},
    support::Str,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AiError, AiResult, Attachment, DocumentAttachment, ImageAttachment,
    attachments::{
        MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, decode_stored_attachments,
        validate_prompt_attachments,
    },
    media_storage,
};

pub(crate) const DEFAULT_DISK: &str = "local";
const DIRECTORY: &str = "ai/conversations/attachments";

#[derive(Clone, Deserialize, Serialize)]
struct AttachmentReference {
    disk: String,
    path: String,
    name: Option<String>,
    mime: String,
    bytes: usize,
    sha256: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct StoredFiles(Vec<AttachmentReference>);

impl StoredFiles {
    pub(crate) fn json(&self) -> AiResult<String> {
        serde_json::to_string(&self.0).map_err(|_| AiError::MediaStorageUnavailable)
    }

    pub(crate) async fn discard(&self) {
        for reference in &self.0 {
            if let Ok(disk) = Storage::disk(Some(&reference.disk)) {
                let _ = disk.delete(&reference.path).await;
            }
        }
    }
}

pub(crate) fn valid_disk(disk: &str) -> bool {
    !disk.trim().is_empty() && disk == disk.trim() && !matches!(disk, "public" | "web")
}

pub(crate) async fn write(
    disk: &str,
    record: &str,
    attachments: &[Attachment],
) -> AiResult<StoredFiles> {
    validate_prompt_attachments(attachments, &[])?;
    let mut files = StoredFiles(Vec::with_capacity(attachments.len()));
    if attachments.is_empty() {
        return Ok(files);
    }
    let directory = directory(record)?;
    if !valid_disk(disk) {
        return Err(AiError::MediaStorageUnavailable);
    }
    let storage = Storage::disk(Some(disk)).map_err(|_| AiError::MediaStorageUnavailable)?;
    for attachment in attachments {
        let (content, name, mime) = match attachment {
            Attachment::Image(image) => (image.content(), None, image.mime()),
            Attachment::Document(document) => (
                document.content(),
                Some(document.filename().to_owned()),
                document.mime(),
            ),
        };
        let path = format!("{directory}/{}", Str::ulid());
        let reference = AttachmentReference {
            disk: disk.to_owned(),
            path,
            name,
            mime: mime.to_owned(),
            bytes: content.len(),
            sha256: format!("{:x}", Sha256::digest(content)),
        };
        let written = storage
            .put_with_options(
                &reference.path,
                content,
                WriteOptions {
                    visibility: Some(Visibility::Private),
                },
            )
            .await;
        files.0.push(reference);
        if !matches!(written, Ok(true)) {
            files.discard().await;
            return Err(AiError::MediaStorageUnavailable);
        }
    }
    Ok(files)
}

pub(crate) async fn read(record: &str, json: &str) -> AiResult<Vec<Attachment>> {
    if is_inline(json)? {
        return decode_stored_attachments(json);
    }
    let files: StoredFiles =
        serde_json::from_str(json).map_err(|_| AiError::MediaStorageUnavailable)?;
    files.read(record).await
}

impl StoredFiles {
    pub(crate) async fn read(&self, record: &str) -> AiResult<Vec<Attachment>> {
        if self.0.is_empty() {
            return Ok(Vec::new());
        }
        if self.0.len() > MAX_ATTACHMENTS {
            return Err(AiError::TooManyAttachments);
        }
        if self
            .0
            .iter()
            .fold(0usize, |sum, file| sum.saturating_add(file.bytes))
            >= MAX_ATTACHMENT_BYTES
        {
            return Err(AiError::MediaTooLarge);
        }
        let directory = directory(record)?;
        let prefix = format!("{directory}/");
        let mut attachments = Vec::with_capacity(self.0.len());
        for reference in &self.0 {
            let key = reference
                .path
                .strip_prefix(&prefix)
                .ok_or(AiError::MediaStorageUnavailable)?;
            if key.len() != 26
                || !key.bytes().all(|byte| byte.is_ascii_alphanumeric())
                || !valid_disk(&reference.disk)
                || reference.bytes == 0
            {
                return Err(AiError::MediaStorageUnavailable);
            }
            let bytes =
                media_storage::read(&reference.path, Some(&reference.disk), reference.bytes + 1)
                    .await?
                    .ok_or(AiError::MediaStorageUnavailable)?;
            if bytes.len() != reference.bytes
                || format!("{:x}", Sha256::digest(&bytes)) != reference.sha256
            {
                return Err(AiError::MediaStorageUnavailable);
            }
            let attachment = match &reference.name {
                Some(name) => {
                    let document = DocumentAttachment::from_bytes(name.as_str(), bytes)?;
                    if document.mime() != reference.mime {
                        return Err(AiError::MediaStorageUnavailable);
                    }
                    document.into()
                }
                None => {
                    let image = ImageAttachment::from_bytes(bytes)?;
                    if image.mime() != reference.mime {
                        return Err(AiError::MediaStorageUnavailable);
                    }
                    image.into()
                }
            };
            attachments.push(attachment);
        }
        Ok(attachments)
    }
}

pub(crate) fn is_inline(json: &str) -> AiResult<bool> {
    // 检查旧记录的类型时跳过附件值，避免额外解码一份 Base64。
    #[derive(Deserialize)]
    struct Shape<'a> {
        #[serde(rename = "type")]
        kind: Option<&'a str>,
    }

    let entries: Vec<Shape<'_>> =
        serde_json::from_str(json).map_err(|_| AiError::MediaStorageUnavailable)?;
    Ok(entries
        .iter()
        .any(|entry| matches!(entry.kind, Some("base64-image" | "base64-document"))))
}

fn directory(record: &str) -> AiResult<String> {
    if record.is_empty()
        || record.len() > 40
        || !record
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(AiError::MediaStorageUnavailable);
    }
    Ok(format!("{DIRECTORY}/{record}"))
}
