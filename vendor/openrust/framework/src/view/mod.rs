//! View-facing framework services.

use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use ::http::{HeaderValue, header::CONTENT_TYPE};

use crate::{
    config::{ConfigError, Value},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
};

pub mod middleware;

#[derive(Clone)]
pub(crate) struct ViewRoot(Arc<[PathBuf]>);

impl ViewRoot {
    pub(crate) fn new(base_path: &Path) -> Self {
        Self([base_path.join("resources/views")].into())
    }

    pub(crate) fn configure(
        &mut self,
        base_path: &Path,
        paths: Option<Value>,
    ) -> Result<(), ConfigError> {
        let Some(paths) = paths else {
            return Ok(());
        };
        let invalid = || ConfigError::InvalidType {
            key: "view.paths".to_owned(),
            expected: "an array of non-empty directory paths",
        };
        let paths = paths.as_array().ok_or_else(invalid)?;
        let paths = paths
            .iter()
            .map(|path| {
                path.as_str()
                    .filter(|path| !path.trim().is_empty())
                    .map(|path| base_path.join(path))
                    .ok_or_else(invalid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.0 = paths.into();
        Ok(())
    }

    fn read(&self, relative: &Path) -> io::Result<String> {
        for directory in self.0.iter() {
            let result = directory.canonicalize().and_then(|root| {
                let path = root.join(relative).canonicalize()?;
                if !path.starts_with(&root) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "View is outside the view directory.",
                    ));
                }
                fs::read_to_string(path)
            });
            match result {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                result => return result,
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "View not found."))
    }
}

/// A named view to be rendered when the HTTP response is prepared.
pub struct View {
    name: Cow<'static, str>,
}

/// Creates a named view from the current application's view directory.
///
/// Dot-separated names map to directories. The current renderer serves UTF-8 HTML files.
pub fn view(name: impl Into<Cow<'static, str>>) -> View {
    View { name: name.into() }
}

impl View {
    pub(crate) fn render(
        self,
        request: &mut Request,
    ) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let root = request
                .parts()
                .extensions
                .get::<ViewRoot>()
                .cloned()
                .ok_or_else(|| HttpError::Internal {
                    source: io::Error::other("No view directory is available for this request.")
                        .into(),
                })?;
            let path = view_path(&self.name).map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
            let html = tokio::task::spawn_blocking(move || root.read(&path))
                .await
                .map_err(|source| HttpError::Internal {
                    source: source.into(),
                })?
                .map_err(|source| HttpError::Internal {
                    source: source.into(),
                })?;

            let mut response = Response::new(StatusCode::OK, Body::from(html));
            response.parts_mut().headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            Ok(response)
        })
    }
}

fn view_path(name: &str) -> io::Result<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split('.') {
        if part.is_empty()
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid view name.",
            ));
        }
        path.push(part);
    }
    path.set_extension("html");
    Ok(path)
}
