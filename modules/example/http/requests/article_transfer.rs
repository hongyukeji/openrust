//! 文件格式是显式协议；不以客户端文件名推断格式或对象路径。

use openrust::http::axum::http::{HeaderMap, StatusCode, header};
use openrust::{Error, Result};

pub(crate) const MAX_TRANSFER_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_TRANSFER_ROWS: u64 = 100;

#[derive(Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArticleFormat {
    #[default]
    Csv,
    Xlsx,
}
impl ArticleFormat {
    pub(crate) fn mime(self) -> &'static str {
        match self {
            Self::Csv => "text/csv",
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        }
    }
    pub(crate) fn disposition(self) -> &'static str {
        match self {
            Self::Csv => "attachment; filename=\"articles.csv\"",
            Self::Xlsx => "attachment; filename=\"articles.xlsx\"",
        }
    }
    pub(crate) fn validate_headers(self, headers: &HeaderMap) -> Result<()> {
        if headers.get_all(header::CONTENT_TYPE).iter().count() != 1 {
            return Err(invalid_type());
        }
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(invalid_type)?;
        let mut parts = content_type.split(';').map(str::trim);
        if !parts
            .next()
            .is_some_and(|value| value.eq_ignore_ascii_case(self.mime()))
        {
            return Err(invalid_type());
        }
        if let Some(parameter) = parts.next()
            && (!matches!(self, Self::Csv)
                || !parameter.eq_ignore_ascii_case("charset=utf-8")
                || parts.next().is_some())
        {
            return Err(invalid_type());
        }
        if let Some(length) = headers.get(header::CONTENT_LENGTH) {
            let length = length
                .to_str()
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| invalid("example_transfer_length_invalid", "文件长度声明无效"))?;
            if length > MAX_TRANSFER_BYTES {
                return Err(too_large());
            }
        }
        Ok(())
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransferQuery {
    #[serde(default)]
    pub(crate) format: ArticleFormat,
}
pub(crate) fn invalid(code: &'static str, message: &'static str) -> Error {
    Error::custom_with_status(StatusCode::BAD_REQUEST, code, message)
}
pub(crate) fn too_large() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "example_transfer_too_large",
        "文章文件超过大小上限",
    )
}
fn invalid_type() -> Error {
    Error::custom_with_status(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "example_transfer_type_invalid",
        "文件类型与所选 CSV/XLSX 格式不符",
    )
}
