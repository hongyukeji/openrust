use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cap_std::{ambient_authority, fs::Dir};
use http::{
    HeaderMap, HeaderValue,
    header::{CONTENT_LENGTH, CONTENT_TYPE, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED},
};
use tokio::io::AsyncReadExt as _;
use tokio_util::io::ReaderStream;

use crate::http::{Body, Method, Response, StatusCode};

use super::RoutingError;

pub(super) struct StaticMount {
    prefix: String,
    directory: Arc<Path>,
    public_disk: bool,
}

impl StaticMount {
    pub(super) fn new(prefix: &str, directory: &Path) -> Result<Self, RoutingError> {
        let uri: http::uri::PathAndQuery =
            prefix.parse().map_err(|_| RoutingError::InvalidUri {
                uri: prefix.to_owned(),
            })?;
        let path = prefix.strip_prefix('/').unwrap_or_default();
        let path = path.trim_end_matches('/');
        if !prefix.starts_with('/')
            || uri.query().is_some()
            || prefix.contains(['%', '#'])
            || (!path.is_empty() && !valid_relative_path(path))
        {
            return Err(RoutingError::InvalidUri { uri: prefix.into() });
        }
        if directory.as_os_str().is_empty() {
            return Err(RoutingError::EmptyStaticDirectory);
        }
        Ok(Self {
            prefix: prefix.trim_end_matches('/').into(),
            directory: Arc::from(directory),
            public_disk: false,
        })
    }

    pub(super) fn public_disk(prefix: &str, directory: &Path) -> Result<Self, RoutingError> {
        let mut mount = Self::new(prefix, directory)?;
        mount.public_disk = true;
        Ok(mount)
    }

    pub(super) fn resolve_directory(&mut self, base: &Path) {
        self.directory = base.join(&self.directory).into();
    }

    pub(super) async fn serve(
        &self,
        path: &str,
        method: &Method,
        headers: &HeaderMap,
    ) -> Option<Response> {
        let relative = path.strip_prefix(&self.prefix)?.strip_prefix('/')?;
        if !valid_relative_path(relative) {
            return None;
        }
        let mime = mime_guess::from_path(relative).first_or_octet_stream();
        let relative = PathBuf::from(relative);
        let directory = Arc::clone(&self.directory);
        let public_disk = self.public_disk;
        let (file, length, modified) = tokio::task::spawn_blocking(move || {
            let root = Dir::open_ambient_dir(directory.as_ref(), ambient_authority()).ok()?;
            let file = if public_disk {
                crate::filesystem::open_public_file(&root, &relative).ok()?
            } else {
                if !root.is_file(&relative) {
                    return None;
                }
                root.open(relative).ok()?.into_std()
            };
            let metadata = file.metadata().ok()?;
            metadata.is_file().then(|| {
                let modified = metadata.modified().ok().and_then(http_modification_time);
                (file, metadata.len(), modified)
            })
        })
        .await
        .ok()??;

        let mut response = if not_modified(headers, modified) {
            Response::empty(StatusCode::NOT_MODIFIED)
        } else {
            let body = if *method == Method::HEAD {
                Body::empty()
            } else {
                Body::from_stream(ReaderStream::new(
                    tokio::fs::File::from_std(file).take(length),
                ))
            };
            let mut response = Response::new(StatusCode::OK, body);
            let headers = &mut response.parts_mut().headers;
            headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref()).expect("valid MIME type"),
            );
            response
        };
        if let Some(modified) = modified {
            response.parts_mut().headers.insert(
                LAST_MODIFIED,
                HeaderValue::from_str(&httpdate::fmt_http_date(modified)).expect("valid HTTP date"),
            );
        }
        Some(response)
    }
}

fn http_modification_time(time: SystemTime) -> Option<SystemTime> {
    // HTTP dates have whole-second precision. httpdate supports 1970 through 9999.
    const YEAR_10000: u64 = 253_402_300_800;
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    (seconds < YEAR_10000)
        .then(|| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .flatten()
}

fn not_modified(headers: &HeaderMap, modified: Option<SystemTime>) -> bool {
    // If-None-Match takes precedence. Only the wildcard can match without an ETag.
    if let Some(tag) = headers.get(IF_NONE_MATCH) {
        return tag.to_str().is_ok_and(|tag| tag.trim() == "*");
    }
    let values = headers.get_all(IF_MODIFIED_SINCE);
    let mut values = values.iter();
    let Some(value) = values.next().filter(|_| values.next().is_none()) else {
        return false;
    };
    modified.is_some_and(|modified| {
        value
            .to_str()
            .ok()
            .and_then(|value| httpdate::parse_http_date(value.trim()).ok())
            .is_some_and(|since| modified <= since)
    })
}

fn valid_relative_path(path: &str) -> bool {
    !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|segment| {
            !matches!(segment, "" | "." | "..") && !crate::filesystem::is_staging_name(segment)
        })
}
