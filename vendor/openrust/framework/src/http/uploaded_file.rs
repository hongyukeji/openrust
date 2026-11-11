use std::sync::{Arc, OnceLock};

use axum::body::Bytes;

use crate::filesystem::{FilesystemError, FilesystemResult, Storage, Visibility, WriteOptions};

/// A received upload with untrusted client metadata and bounded file content.
#[derive(Clone, Debug)]
pub struct UploadedFile {
    path: String,
    media_type: Option<String>,
    content: Bytes,
    hash_name: Arc<OnceLock<String>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's uploaded-file API")]
impl UploadedFile {
    pub(super) fn new(path: String, media_type: Option<String>, content: Bytes) -> Self {
        Self {
            path,
            media_type,
            content,
            hash_name: Arc::default(),
        }
    }

    /// The filename supplied by the client. Treat it as display data.
    pub fn getClientOriginalName(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or_default()
    }

    /// The original client path, which may include directories.
    pub fn getClientOriginalPath(&self) -> &str {
        &self.path
    }

    /// The media type supplied by the client.
    pub fn getClientMimeType(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    pub fn getSize(&self) -> usize {
        self.content.len()
    }

    pub fn get(&self) -> Bytes {
        self.content.clone()
    }

    /// Detects a file extension from the received bytes.
    pub fn extension(&self) -> Option<&'static str> {
        if let Some(kind) = infer::get(&self.content) {
            return Some(kind.extension());
        }
        let text = std::str::from_utf8(&self.content).ok()?;
        (!text.is_empty()
            && text.chars().all(|character| {
                !character.is_control() || matches!(character, '\t' | '\n' | '\r' | '\u{c}')
            }))
        .then_some("txt")
    }

    /// Returns the same server-generated basename for this upload and its clones.
    pub fn hashName(&self) -> FilesystemResult<&str> {
        if let Some(name) = self.hash_name.get() {
            return Ok(name);
        }
        let mut name = random_name()?;
        if let Some(extension) = self.extension() {
            name.push('.');
            name.push_str(extension);
        }
        Ok(self.hash_name.get_or_init(|| name))
    }

    pub async fn store(
        &self,
        directory: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        self.storeAs(directory, self.hashName()?, disk).await
    }

    pub async fn storeAs(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        Storage::disk(disk)?
            .putFileAs(directory, self, name, WriteOptions::default())
            .await
    }

    pub async fn storePublicly(
        &self,
        directory: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        self.storePubliclyAs(directory, self.hashName()?, disk)
            .await
    }

    pub async fn storePubliclyAs(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> FilesystemResult<Option<String>> {
        Storage::disk(disk)?
            .putFileAs(
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

fn random_name() -> FilesystemResult<String> {
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
