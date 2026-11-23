use std::{
    fmt,
    sync::{Arc, OnceLock},
};

use axum::body::Bytes;
use indexmap::IndexMap;

use crate::filesystem::{FilesystemError, FilesystemResult, Storage, Visibility, WriteOptions};

/// A file field, a keyed group of uploads, or a missing member in a wildcard result.
#[derive(Clone, Debug)]
pub enum UploadedFileValue {
    File(UploadedFile),
    Files(IndexMap<String, Self>),
    Null,
}

/// A received upload with untrusted client metadata and bounded file content.
#[derive(Clone)]
pub struct UploadedFile {
    path: String,
    media_type: Option<String>,
    content: Bytes,
    metadata: Arc<UploadMetadata>,
}

#[derive(Default)]
struct UploadMetadata {
    detected_type: OnceLock<Option<(&'static str, &'static str)>>,
    hash_name: OnceLock<String>,
}

impl fmt::Debug for UploadedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadedFile")
            .field("path", &self.path)
            .field("media_type", &self.media_type)
            .field("size", &self.content.len())
            .finish_non_exhaustive()
    }
}

impl UploadedFile {
    pub(crate) fn detect_type(content: &[u8]) -> Option<(&'static str, &'static str)> {
        detect_type(content)
    }

    pub(super) fn new(path: String, media_type: Option<String>, content: Bytes) -> Self {
        Self {
            path,
            media_type,
            content,
            metadata: Arc::default(),
        }
    }

    /// The filename supplied by the client. Treat it as display data.
    pub fn get_client_original_name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or_default()
    }

    /// The original client path, which may include directories.
    pub fn get_client_original_path(&self) -> &str {
        &self.path
    }

    /// The media type supplied by the client.
    pub fn get_client_mime_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    pub fn get_size(&self) -> usize {
        self.content.len()
    }

    pub fn get(&self) -> Bytes {
        self.content.clone()
    }

    /// Detects a file extension from the received bytes.
    pub fn extension(&self) -> Option<&'static str> {
        self.detected_type()
            .map(|(extension, _)| extension)
            .filter(|extension| !extension.is_empty())
    }

    pub(crate) fn detected_type(&self) -> Option<(&'static str, &'static str)> {
        *self
            .metadata
            .detected_type
            .get_or_init(|| detect_type(&self.content))
    }

    /// Returns the same server-generated basename for this upload and its clones.
    pub fn hash_name(&self) -> FilesystemResult<&str> {
        if let Some(name) = self.metadata.hash_name.get() {
            return Ok(name);
        }
        let mut name = random_name()?;
        if let Some(extension) = self.extension() {
            name.push('.');
            name.push_str(extension);
        }
        Ok(self.metadata.hash_name.get_or_init(|| name))
    }

    pub async fn store(
        &self,
        directory: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        self.store_as(directory, self.hash_name()?, disk).await
    }

    pub async fn store_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        Storage::disk(disk)?
            .put_file_as(directory, self, name, WriteOptions::default())
            .await
    }

    pub async fn store_publicly(
        &self,
        directory: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        self.store_publicly_as(directory, self.hash_name()?, disk)
            .await
    }

    pub async fn store_publicly_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        Storage::disk(disk)?
            .put_file_as(
                directory,
                self,
                name,
                WriteOptions {
                    visibility: Some(Visibility::Public),
                },
            )
            .await
    }
}

fn detect_type(content: &[u8]) -> Option<(&'static str, &'static str)> {
    if let Some(kind) = infer::get(content) {
        return Some((kind.extension(), kind.mime_type()));
    }
    if content.is_empty() {
        return Some(("", "application/x-empty"));
    }
    let first = content
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace());
    if matches!(first, Some(b'{' | b'['))
        && serde_json::from_slice::<serde::de::IgnoredAny>(content).is_ok()
    {
        return Some(("json", "application/json"));
    }
    let utf16 = content
        .strip_prefix(b"\xff\xfe")
        .map(|text| (text, true))
        .or_else(|| content.strip_prefix(b"\xfe\xff").map(|text| (text, false)));
    if let Some((bytes, little_endian)) = utf16 {
        let (units, remainder) = bytes.as_chunks::<2>();
        let text = units.iter().map(|unit| {
            if little_endian {
                u16::from_le_bytes(*unit)
            } else {
                u16::from_be_bytes(*unit)
            }
        });
        return (remainder.is_empty()
            && char::decode_utf16(text).all(|character| character.is_ok_and(printable)))
        .then_some(("txt", "text/plain"));
    }
    let text = std::str::from_utf8(content).ok()?;
    text.chars().all(printable).then_some(("txt", "text/plain"))
}

fn printable(character: char) -> bool {
    !character.is_control() || matches!(character, '\t' | '\n' | '\r' | '\u{c}')
}

pub(crate) fn random_name() -> FilesystemResult<String> {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut output = String::with_capacity(40);
    while output.len() < 40 {
        let mut bytes = [0_u8; 40];
        aws_lc_rs::rand::fill(&mut bytes).map_err(|source| FilesystemError::Backend {
            source: Box::new(source),
        })?;
        for byte in bytes
            .into_iter()
            .filter(|byte| *byte < 248)
            .take(40 - output.len())
        {
            output.push(char::from(ALPHABET[usize::from(byte % 62)]));
        }
    }
    Ok(output)
}
