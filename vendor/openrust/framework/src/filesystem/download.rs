use http::{
    HeaderValue,
    header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
};
use tokio_util::io::ReaderStream;

use crate::http::{Body, HeaderMap, Response, StatusCode, filename_header};

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
            headers.insert(
                CONTENT_DISPOSITION,
                filename_header(disposition, name).map_err(|_| FilesystemError::InvalidPath)?,
            );
        }
        Ok(Response::new(
            StatusCode::OK,
            Body::from_stream(ReaderStream::new(self.reader)),
        )
        .withHeaders(headers))
    }
}
