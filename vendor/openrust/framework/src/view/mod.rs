//! View-facing framework services.

use std::{
    borrow::Cow, collections::HashMap, future::Future, io, path::PathBuf, pin::Pin, sync::Arc,
};

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
mod request_data;
mod value;
mod view_finder;
mod view_service_provider;

pub use component::{Component, ComponentOutput};
pub use factory::Factory;
pub use file_view_finder::FileViewFinder;
pub(crate) use rendering::{ComponentRenderer, EscapedEcho};
pub use view_finder::ViewFinder;
pub use view_service_provider::ViewServiceProvider;

pub use value::{ViewIndex, ViewNumber, ViewValue};
pub type ViewData = indexmap::IndexMap<String, ViewValue>;
pub type ViewFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ViewResult<T> = Result<T, ViewError>;

type IterationEntries = Box<dyn Iterator<Item = (ViewValue, ViewValue)>>;
type ViewIteration = Arc<dyn Fn() -> (Option<usize>, IterationEntries) + Send + Sync>;

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

/// 已渲染根视图的名称与最终绑定数据，保留创建器、组合器及共享数据的结果。
#[derive(Clone)]
pub(crate) struct RenderedView {
    pub(crate) name: String,
    pub(crate) data: ViewData,
}

/// A named view to be rendered when the HTTP response is prepared.
pub struct View {
    name: Cow<'static, str>,
    data: ViewData,
    binding: Option<Box<ViewBinding>>,
    created: bool,
    creation_error: Option<ViewError>,
    options: Option<Box<ViewOptions>>,
}

#[derive(Default)]
struct ViewOptions {
    fragments: Option<FragmentSelection>,
    iterations: HashMap<String, ViewIteration>,
    gate_arguments: HashMap<String, Arc<crate::auth::access::Argument>>,
}

#[derive(Clone)]
enum FragmentSelection {
    Named(Vec<String>),
    All,
}

struct ViewBinding {
    factory: Factory,
    file: Option<PathBuf>,
}

/// Creates a named view from the current application's view directory.
///
/// Dot-separated names map to directories.
pub fn view(name: impl Into<Cow<'static, str>>) -> View {
    use crate::support::facades::{Facade as _, View as ViewFacade};

    let mut view = View {
        name: name.into(),
        data: ViewData::new(),
        binding: None,
        created: false,
        creation_error: None,
        options: None,
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

    /// Registers a fresh keyed iterator per Blade loop, with unknown length.
    #[allow(non_snake_case, reason = "Matches the Blade view API naming")]
    pub fn withIterator<I, F>(self, key: impl Into<String>, create: F) -> Self
    where
        I: Iterator<Item = (ViewValue, ViewValue)> + 'static,
        F: Fn() -> I + Send + Sync + 'static,
    {
        self.with_iteration(key, move || (None, Box::new(create())))
    }

    /// Registers a fresh keyed iterator whose exact length is available without consuming it.
    #[allow(non_snake_case, reason = "Matches the Blade view API naming")]
    pub fn withExactSizeIterator<I, F>(self, key: impl Into<String>, create: F) -> Self
    where
        I: ExactSizeIterator<Item = (ViewValue, ViewValue)> + 'static,
        F: Fn() -> I + Send + Sync + 'static,
    {
        self.with_iteration(key, move || {
            let entries = create();
            (Some(entries.len()), Box::new(entries))
        })
    }

    fn with_iteration(
        mut self,
        key: impl Into<String>,
        create: impl Fn() -> (Option<usize>, IterationEntries) + Send + Sync + 'static,
    ) -> Self {
        self.options
            .get_or_insert_default()
            .iterations
            .insert(key.into(), Arc::new(create));
        self
    }

    /// Adds a typed value or policy target for authorization directives.
    #[allow(non_snake_case)]
    pub fn withGateArgument<T: Send + Sync + 'static>(
        mut self,
        name: impl Into<String>,
        argument: T,
    ) -> Self {
        self.options
            .get_or_insert_default()
            .gate_arguments
            .insert(name.into(), Arc::new(argument));
        self
    }

    /// Adds a Rust value formatted as text by this application's Blade compiler.
    #[allow(
        non_snake_case,
        reason = "Matches the Blade stringable registration API"
    )]
    pub fn withStringable<T: 'static>(self, key: impl Into<String>, value: &T) -> ViewResult<Self> {
        use crate::support::facades::{Facade as _, View as ViewFacade};

        let formatted = match &self.binding {
            Some(binding) => binding.factory.compiler().applyEchoHandler(value)?,
            None => ViewFacade::getFacadeRoot()
                .map_err(ViewError::Unavailable)?
                .compiler()
                .applyEchoHandler(value)?,
        };
        Ok(self.with(key, formatted))
    }

    /// Selects one Blade fragment for the response.
    pub fn fragment(self, name: impl Into<String>) -> Self {
        self.fragments([name])
    }

    /// Selects Blade fragments in the given output order.
    pub fn fragments(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.options.get_or_insert_default().fragments = Some(FragmentSelection::Named(
            names.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Selects every captured Blade fragment.
    #[allow(non_snake_case, reason = "Matches Laravel's View API")]
    pub fn allFragments(mut self) -> Self {
        self.options.get_or_insert_default().fragments = Some(FragmentSelection::All);
        self
    }

    /// Selects a fragment when the caller's condition is true.
    #[allow(non_snake_case, reason = "Matches Laravel's View API")]
    pub fn fragmentIf(self, condition: bool, name: impl Into<String>) -> Self {
        self.fragmentsIf(condition, [name])
    }

    /// Selects fragments when the caller's condition is true.
    #[allow(non_snake_case, reason = "Matches Laravel's View API")]
    pub fn fragmentsIf(
        mut self,
        condition: bool,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.options.get_or_insert_default().fragments = condition
            .then(|| FragmentSelection::Named(names.into_iter().map(Into::into).collect()));
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
            let locale = crate::translation::LocaleSnapshot::capture();
            let scope = crate::container::ScopeSnapshot::capture();
            let errors = middleware::shared_errors();
            let (check_sender, mut checks) = tokio::sync::mpsc::channel(1);
            let request_data = request_data::ViewRequestData::capture(request, check_sender);
            let mut rendering = tokio::task::spawn_blocking(move || {
                middleware::render_scope(errors, || {
                    locale.run(|| {
                        scope.run(|| context.run(|| factory.render_in_request(self, request_data)))
                    })
                })
            });
            let rendered = loop {
                tokio::select! {
                    result = &mut rendering => break result,
                    Some(query) = checks.recv() => query.resolve(request).await,
                }
            };
            let (html, original) = rendered
                .map_err(|source| HttpError::Internal {
                    source: source.into(),
                })?
                .map_err(|source| HttpError::Internal {
                    source: source.into(),
                })?;

            let mut response = Response::new(StatusCode::OK, Body::from(html));
            response.parts_mut().extensions.insert(Arc::new(original));
            response.parts_mut().headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            Ok(response)
        })
    }
}
