//! 附件上传声明；文件名和对象 key 从不取自客户端。

use openrust::http::axum::http::{HeaderMap, StatusCode, header};
use openrust::{Error, Result};

pub(crate) const MAX_ATTACHMENT_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_ATTACHMENT_CHUNK: usize = 16 * 1024;

/// 只允许 UTF-8 纯文本；声明本身不能替代流内容检查。
#[derive(Clone, Copy)]
pub(crate) struct AttachmentUpload {
    _validated: (),
}

impl AttachmentUpload {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Result<Self> {
        if headers.get_all(header::CONTENT_TYPE).iter().count() != 1 {
            return Err(invalid_type());
        }
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(invalid_type)?;
        Self::from_content_type(content_type)?;
        if let Some(value) = headers.get(header::CONTENT_LENGTH) {
            let bytes = value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| {
                    Error::custom_with_status(
                        StatusCode::BAD_REQUEST,
                        "example_attachment_length_invalid",
                        "附件长度声明无效",
                    )
                })?;
            if bytes > MAX_ATTACHMENT_BYTES {
                return Err(too_large());
            }
        }
        Ok(Self { _validated: () })
    }

    pub(crate) fn from_content_type(value: &str) -> Result<Self> {
        let mut parts = value.split(';').map(str::trim);
        if !parts
            .next()
            .is_some_and(|part| part.eq_ignore_ascii_case("text/plain"))
        {
            return Err(invalid_type());
        }
        if let Some(parameter) = parts.next()
            && (!parameter.eq_ignore_ascii_case("charset=utf-8") || parts.next().is_some())
        {
            return Err(invalid_type());
        }
        Ok(Self { _validated: () })
    }
}

pub(crate) fn too_large() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "example_attachment_too_large",
        "附件超过大小上限",
    )
}
fn invalid_type() -> Error {
    Error::custom_with_status(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "example_attachment_type_invalid",
        "附件仅支持 UTF-8 纯文本",
    )
}
