use std::{io, ops::Range, path::Path, time::UNIX_EPOCH};

use futures_util::{TryStreamExt as _, stream};
use http::{
    HeaderMap, HeaderName, HeaderValue, Method,
    header::{
        ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
        IF_RANGE, LAST_MODIFIED, RANGE, TRANSFER_ENCODING,
    },
};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncReadExt as _, AsyncSeekExt as _, SeekFrom},
};
use tokio_util::io::ReaderStream;

use super::{
    Body, HttpError, Response, StatusCode, filename_header, middleware::set_public_cache_control,
};

pub(super) struct BinaryFile {
    file: File,
    length: u64,
}

impl Response {
    /// Streams a regular local file from a path chosen by the application.
    pub async fn file(path: impl AsRef<Path>, headers: HeaderMap) -> Result<Self, HttpError> {
        BinaryFile::open(path.as_ref(), headers).await
    }

    /// Streams a local file with a download name and `attachment` or `inline` disposition.
    pub async fn download(
        path: impl AsRef<Path>,
        name: Option<&str>,
        mut headers: HeaderMap,
        disposition: &str,
    ) -> Result<Self, HttpError> {
        let path = path.as_ref();
        let default = path.file_name().map(|name| name.to_string_lossy());
        let name = name
            .filter(|name| !name.is_empty())
            .or(default.as_deref())
            .ok_or(HttpError::InvalidRequest)?;
        headers.insert(CONTENT_DISPOSITION, filename_header(disposition, name)?);
        BinaryFile::open(path, headers).await
    }

    pub(crate) fn prepare_binary_file(&mut self, method: &Method, request: &HeaderMap) {
        let Some(BinaryFile { mut file, length }) = self.binary_file.take() else {
            return;
        };
        self.content = None;
        if !self.status().is_success() {
            return;
        }

        let parts = self.parts_mut();
        let headers = &mut parts.headers;
        headers.remove(TRANSFER_ENCODING);
        headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
        headers.entry(ACCEPT_RANGES).or_insert_with(|| {
            HeaderValue::from_static(if method.is_safe() { "bytes" } else { "none" })
        });

        let mut selection = 0..length;
        if method == Method::GET
            && if_range_matches(request, headers)
            && let Some(range) = single_header(request, &RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| byte_range(value, length))
        {
            match range {
                ByteRange::Unsatisfiable => {
                    parts.status = StatusCode::RANGE_NOT_SATISFIABLE;
                    headers.insert(CONTENT_LENGTH, HeaderValue::from(0));
                    headers.insert(CONTENT_RANGE, header_value(&format!("bytes */{length}")));
                    return;
                }
                ByteRange::Selection(range) if range != selection => {
                    parts.status = StatusCode::PARTIAL_CONTENT;
                    headers.insert(
                        CONTENT_RANGE,
                        header_value(&format!("bytes {}-{}/{length}", range.start, range.end - 1)),
                    );
                    headers.insert(CONTENT_LENGTH, HeaderValue::from(range.end - range.start));
                    selection = range;
                }
                ByteRange::Selection(_) => {}
            }
        }

        if method != Method::HEAD && !selection.is_empty() {
            let stream = stream::once(async move {
                if selection.start != 0 {
                    file.seek(SeekFrom::Start(selection.start)).await?;
                }
                Ok::<_, io::Error>(ReaderStream::new(
                    file.take(selection.end - selection.start),
                ))
            });
            self.body = Body::from_stream(stream.try_flatten());
        }
    }
}

impl BinaryFile {
    async fn open(path: &Path, mut headers: HeaderMap) -> Result<Response, HttpError> {
        let mut options = OpenOptions::new();
        options.read(true);
        // Opening a FIFO on Unix must not block before the regular-file check.
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = options.open(path).await.map_err(file_error)?;
        let metadata = file.metadata().await.map_err(file_error)?;
        if !metadata.is_file() {
            return Err(file_error(io::Error::other(
                "HTTP file response requires a regular file",
            )));
        }
        if let Ok(modified) = metadata.modified()
            && modified
                .duration_since(UNIX_EPOCH)
                .is_ok_and(|elapsed| elapsed.as_secs() < 253_402_300_800)
        {
            headers.insert(
                LAST_MODIFIED,
                header_value(&httpdate::fmt_http_date(modified)),
            );
        }
        let content_type = headers.entry(CONTENT_TYPE).or_insert_with(|| {
            header_value(
                mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .essence_str(),
            )
        });
        if let Ok(value) = content_type.to_str()
            && value.to_ascii_lowercase().starts_with("text/")
            && !value.to_ascii_lowercase().contains("charset")
        {
            *content_type = header_value(&format!("{value}; charset=utf-8"));
        }
        set_public_cache_control(&mut headers);
        let mut response = Response::empty(StatusCode::OK).withHeaders(headers);
        response.binary_file = Some(Self {
            file,
            length: metadata.len(),
        });
        Ok(response)
    }
}

enum ByteRange {
    Selection(Range<u64>),
    Unsatisfiable,
}

fn byte_range(header: &str, length: u64) -> Option<ByteRange> {
    let (start, end) = header.strip_prefix("bytes=")?.split_once('-')?;
    let (start, end) = if start.is_empty() {
        let suffix = parse_digits(end)?;
        if suffix == 0 || suffix > length {
            return Some(ByteRange::Unsatisfiable);
        }
        (length - suffix, length)
    } else {
        let start = parse_digits(start)?;
        let end = if end.is_empty() {
            length
        } else {
            let last = parse_digits(end)?;
            if last < start {
                return None;
            }
            last.saturating_add(1).min(length)
        };
        (start, end)
    };
    Some(if start < end {
        ByteRange::Selection(start..end)
    } else {
        ByteRange::Unsatisfiable
    })
}

fn parse_digits(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    Some(value.bytes().fold(0_u64, |number, digit| {
        number
            .saturating_mul(10)
            .saturating_add(u64::from(digit - b'0'))
    }))
}

fn if_range_matches(request: &HeaderMap, response: &HeaderMap) -> bool {
    if !request.contains_key(IF_RANGE) {
        return true;
    }
    let Some(value) = single_header(request, &IF_RANGE) else {
        return false;
    };
    if value.as_bytes().starts_with(b"\"") {
        return response.get(ETAG) == Some(value);
    }
    response.get(LAST_MODIFIED) == Some(value)
}

fn single_header<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a HeaderValue> {
    let all = headers.get_all(name);
    let mut values = all.iter();
    values.next().filter(|_| values.next().is_none())
}

fn file_error(source: io::Error) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

fn header_value(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).expect("valid file response header")
}
