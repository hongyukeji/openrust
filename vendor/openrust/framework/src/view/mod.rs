//! View-facing framework services.

use std::{borrow::Cow, future::Future, io, pin::Pin, sync::Arc};

use ::http::{HeaderValue, header::CONTENT_TYPE};

use crate::http::{Body, HttpError, HttpFuture, Request, Response, StatusCode};

pub mod compilers;
mod component;
pub(crate) mod console;
pub mod engines;
mod factory;
mod file_view_finder;
pub mod middleware;
mod rendering;
mod view_finder;
mod view_service_provider;

pub use component::{Component, ComponentOutput};
pub use factory::Factory;
pub use file_view_finder::FileViewFinder;
pub use view_finder::ViewFinder;
pub use view_service_provider::ViewServiceProvider;

pub type ViewValue = serde_json::Value;
pub type ViewData = serde_json::Map<String, ViewValue>;
pub type ViewFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ViewResult<T> = Result<T, ViewError>;

/// A lookup, compilation, or rendering failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum ViewError {
    InvalidName,
    NotFound,
    OutsideRoot,
    UnknownEngine,
    Unavailable(crate::contracts::container::BindingResolutionError),
    ReadFailed(io::Error),
    CompileFailed(Box<dyn std::error::Error + Send + Sync>),
    RenderFailed(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidName => "Invalid view name.",
            Self::NotFound => "View not found.",
            Self::OutsideRoot => "View is outside the view directory.",
            Self::UnknownEngine => "View engine is unavailable.",
            Self::Unavailable(_) => "View factory is unavailable.",
            Self::ReadFailed(_) => "Could not read the view.",
            Self::CompileFailed(_) => "Could not compile the view.",
            Self::RenderFailed(_) => "Could not render the view.",
        })
    }
}

impl std::error::Error for ViewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadFailed(source) => Some(source),
            Self::Unavailable(source) => Some(source),
            Self::CompileFailed(source) | Self::RenderFailed(source) => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// A named view to be rendered when the HTTP response is prepared.
pub struct View {
    name: Cow<'static, str>,
    data: ViewData,
    created: bool,
    creation_error: Option<ViewError>,
}

/// Creates a named view from the current application's view directory.
///
/// Dot-separated names map to directories.
pub fn view(name: impl Into<Cow<'static, str>>) -> View {
    use crate::support::facades::{Facade as _, View as ViewFacade};

    let mut view = View {
        name: name.into(),
        data: ViewData::new(),
        created: false,
        creation_error: None,
    };
    if let Ok(factory) = ViewFacade::getFacadeRoot() {
        view.creation_error = factory.callCreator(&mut view).err();
    }
    view
}

impl View {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn data(&self) -> &ViewData {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<ViewValue>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    pub(crate) fn render(
        self,
        request: &mut Request,
    ) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let factory = request
                .parts()
                .extensions
                .get::<Arc<Factory>>()
                .cloned()
                .ok_or_else(|| HttpError::Internal {
                    source: io::Error::other("No view factory is available for this request.")
                        .into(),
                })?;
            let context = crate::support::facades::context_snapshot();
            let html =
                tokio::task::spawn_blocking(move || context.run(|| factory.renderView(self)))
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
