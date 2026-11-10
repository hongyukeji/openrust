//! HTTP request lifecycle for a bootstrapped application.

mod form_request;
mod health_check;
pub mod middleware;
mod server;

pub use form_request::{FormRequest, FormRequestError};
pub use health_check::HealthCheck;

use axum::body::HttpBody as _;

use super::{Application, BootstrapError, configuration::Middleware};
use crate::{
    contracts::{foundation::Application as _, http},
    http::{HttpFuture, Method, Next, Request, RequestParts, Response, ResponseParts, StatusCode},
    view::ViewRoot,
};

/// Dispatches requests and coordinates middleware and error rendering.
pub struct Kernel {
    application: Application,
    middleware: Middleware,
    view_root: ViewRoot,
    ready: bool,
}

impl Kernel {
    pub fn new(mut application: Application) -> Self {
        let mut middleware = Middleware::default();
        for configure in std::mem::take(&mut application.middleware_configurators) {
            configure(&mut middleware);
        }
        Self {
            view_root: ViewRoot::new(application.base_path()),
            application,
            middleware,
            ready: false,
        }
    }
}

impl http::Kernel for Kernel {
    fn bootstrap(&mut self) -> HttpFuture<'_, Result<(), BootstrapError>> {
        Box::pin(async move {
            if !self.ready {
                self.application.boot().await?;
                self.view_root
                    .configure(
                        self.application.base_path(),
                        self.application.config().get("view.paths"),
                    )
                    .map_err(|source| BootstrapError::ConfigurationFailed { source })?;
                self.application
                    .routes
                    .configure_middleware(&self.middleware.registry)
                    .map_err(|source| BootstrapError::RoutingFailed { source })?;
                self.ready = true;
            }
            Ok(())
        })
    }

    fn get_application(&self) -> &Application {
        &self.application
    }

    fn handle<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Response> {
        Box::pin(async move {
            if !self.ready {
                return Response::empty(StatusCode::SERVICE_UNAVAILABLE);
            }
            let previous_exceptions = request
                .exceptions
                .replace(self.application.exceptions.clone());
            let previous_view_root = request
                .parts_mut()
                .extensions
                .insert(self.view_root.clone());
            let head = request.method() == Method::HEAD;
            let mut response = Next::new(&self.middleware.global, &self.application.routes)
                .run(request)
                .await;
            request.exceptions = previous_exceptions;
            let extensions = &mut request.parts_mut().extensions;
            if let Some(view_root) = previous_view_root {
                extensions.insert(view_root);
            } else {
                extensions.remove::<ViewRoot>();
            }
            if head {
                let body = response.take_body();
                if response.status() != StatusCode::NOT_MODIFIED
                    && let Some(length) = body.size_hint().exact()
                {
                    response
                        .parts_mut()
                        .headers
                        .entry(::http::header::CONTENT_LENGTH)
                        .or_insert_with(|| ::http::HeaderValue::from(length));
                }
            }
            response
        })
    }

    fn terminate<'a>(
        &'a self,
        _request: &'a RequestParts,
        _response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(async {})
    }
}
