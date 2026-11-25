use serde::{Deserialize, Serialize};

use crate::{ModerationError, ModerationResult};

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_INLINE_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// 传入原文，驱动按供应商能力进一步限制字符数并选择模型。
pub struct TextInput {
    text: String,
    language: Option<String>,
}

impl TextInput {
    pub fn new(text: impl Into<String>) -> ModerationResult<Self> {
        let text = text.into();
        if text.len() > MAX_TEXT_BYTES || text.trim().is_empty() {
            return Err(ModerationError::InvalidInput("text"));
        }
        Ok(Self {
            text,
            language: None,
        })
    }

    pub fn with_language(mut self, language: impl Into<String>) -> ModerationResult<Self> {
        let language = language.into();
        if !valid_language_hint(&language) {
            return Err(ModerationError::InvalidInput("language"));
        }
        self.language = Some(language);
        Ok(self)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
}

pub(crate) fn valid_language_hint(language: &str) -> bool {
    language.len() <= 35
        && language
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

/// 私有素材的逻辑引用。驱动须通过配置的存储读取并核对版本，不直接以路径访问文件系统。
#[derive(Clone, Eq, PartialEq)]
pub struct StoredMedia {
    disk: String,
    key: String,
    version: String,
}

impl StoredMedia {
    pub fn new(
        disk: impl Into<String>,
        key: impl Into<String>,
        version: impl Into<String>,
    ) -> ModerationResult<Self> {
        let disk = checked_identifier(disk.into(), 128, "disk")?;
        let key = checked_identifier(key.into(), 1024, "object key")?;
        let version = checked_identifier(version.into(), 256, "object version")?;
        Ok(Self { disk, key, version })
    }

    pub fn disk(&self) -> &str {
        &self.disk
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

enum ImageSource {
    Inline(Vec<u8>),
    Stored(StoredMedia),
}

pub struct ImageInput(ImageSource);

impl ImageInput {
    pub fn inline(bytes: Vec<u8>) -> ModerationResult<Self> {
        if bytes.is_empty() || bytes.len() > MAX_INLINE_IMAGE_BYTES {
            return Err(ModerationError::InvalidInput("image bytes"));
        }
        Ok(Self(ImageSource::Inline(bytes)))
    }

    pub fn stored(source: StoredMedia) -> Self {
        Self(ImageSource::Stored(source))
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.0 {
            ImageSource::Inline(bytes) => Some(bytes),
            ImageSource::Stored(_) => None,
        }
    }

    pub fn as_stored(&self) -> Option<&StoredMedia> {
        match &self.0 {
            ImageSource::Inline(_) => None,
            ImageSource::Stored(source) => Some(source),
        }
    }
}

pub struct VideoInput(StoredMedia);

impl VideoInput {
    pub fn new(source: StoredMedia) -> Self {
        Self(source)
    }

    pub fn source(&self) -> &StoredMedia {
        &self.0
    }
}

/// 可跨进程保存的供应商任务标识。调用方还须保留关联的项目、配置及凭据修订。
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TaskId(String);

impl TaskId {
    pub fn new(value: impl Into<String>) -> ModerationResult<Self> {
        checked_identifier(value.into(), 256, "task ID").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TaskId {
    type Error = ModerationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TaskId> for String {
    fn from(value: TaskId) -> Self {
        value.0
    }
}

fn checked_identifier(
    value: String,
    max_bytes: usize,
    field: &'static str,
) -> ModerationResult<String> {
    if value.len() > max_bytes || value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(ModerationError::InvalidInput(field));
    }
    Ok(value)
}
