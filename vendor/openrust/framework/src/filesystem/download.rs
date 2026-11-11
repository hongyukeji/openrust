use http::{
    HeaderValue,
    header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use tokio_util::io::ReaderStream;

use crate::http::{HeaderMap, Response, StatusCode};

use super::{FilesystemError, FilesystemReader, FilesystemResult};

pub(super) struct OpenedFile {
    pub reader: FilesystemReader,
    pub length: Option<u64>,
    pub content_type: Option<String>,
}

impl OpenedFile {
    pub(super) fn response(
        self,
        path: &str,
        name: Option<&str>,
        mut headers: HeaderMap,
        disposition: &str,
    ) -> FilesystemResult<Response> {
        if let Some(length) = self.length {
            headers
                .entry(CONTENT_LENGTH)
                .or_insert_with(|| length.into());
        }
        headers.entry(CONTENT_TYPE).or_insert_with(|| {
            self.content_type
                .as_deref()
                .and_then(|value| HeaderValue::from_str(value).ok())
                .unwrap_or_else(|| {
                    let mime = mime_guess::from_path(path).first_or_octet_stream();
                    HeaderValue::from_str(mime.essence_str()).expect("valid MIME type")
                })
        });
        if !headers.contains_key(CONTENT_DISPOSITION) {
            let name = name.unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
            headers.insert(CONTENT_DISPOSITION, filename_header(disposition, name)?);
        }
        Ok(Response::stream(StatusCode::OK, ReaderStream::new(self.reader)).withHeaders(headers))
    }
}

fn filename_header(disposition: &str, name: &str) -> FilesystemResult<HeaderValue> {
    if name.is_empty() || name.contains(['/', '\\']) || name.chars().any(char::is_control) {
        return Err(FilesystemError::InvalidPath);
    }
    let mut fallback = deunicode::deunicode_with_tofu(name, "");
    fallback.retain(|character| !matches!(character, '%' | '/' | '\\') && !character.is_control());
    if fallback.is_empty() {
        fallback = "_".repeat(name.chars().count());
    }
    let extended = fallback != name;
    if !fallback
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&'*+-.^_`|~".contains(&byte))
    {
        fallback = format!("\"{}\"", fallback.replace('"', "\\\""));
    }
    let mut value = format!("{disposition}; filename={fallback}");
    if extended {
        use std::fmt::Write as _;
        const ENCODE: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        write!(
            value,
            "; filename*=utf-8''{}",
            utf8_percent_encode(name, ENCODE)
        )
        .expect("writing to String");
    }
    HeaderValue::from_str(&value).map_err(|_| FilesystemError::InvalidPath)
}
