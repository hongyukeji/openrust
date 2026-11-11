//! HTTP request lifecycle for a bootstrapped application.

mod form_request;
mod health_check;
pub mod middleware;
mod server;

pub use form_request::{FormRequest, FormRequestError};
pub use health_check::HealthCheck;

use std::sync::Arc;

use axum::body::HttpBody as _;

use super::{Application, BootstrapError, configuration::Middleware};
use crate::{
    contracts::{container::Container as _, foundation::Application as _, http},
    http::{HttpFuture, Method, Next, Request, RequestParts, Response, ResponseParts, StatusCode},
    view::Factory,
};

/// Dispatches requests and coordinates middleware and error rendering.
pub struct Kernel {
    application: Application,
    middleware: Middleware,
    views: Option<Arc<Factory>>,
    ready: bool,
    routes: Option<crate::routing::Router>,
}

impl Kernel {
    pub fn new(application: Application) -> Self {
        let mut middleware = Middleware::default();
        for configure in std::mem::take(
            &mut *application
                .shared
                .middleware_configurators
                .lock()
                .expect("middleware configuration lock poisoned"),
        ) {
            configure(&mut middleware);
        }
        Self {
            views: None,
            application,
            middleware,
            ready: false,
            routes: None,
        }
    }
}

impl http::Kernel for Kernel {
    fn bootstrap(&mut self) -> HttpFuture<'_, Result<(), BootstrapError>> {
        Box::pin(async move {
            if !self.ready {
                self.application.boot().await?;
                self.views = Some(
                    self.application
                        .container()
                        .get::<Factory>(crate::container::ServiceKey::typed::<Factory>())
                        .map_err(|source| BootstrapError::ContainerFailed { source })?,
                );
                if self.routes.is_none() {
                    self.routes = self
                        .application
                        .shared
                        .routes
                        .lock()
                        .expect("application routes lock poisoned")
                        .take();
                }
                self.routes
                    .as_mut()
                    .expect("boot assembled the application routes")
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
        Box::pin(crate::support::facades::scope(
            self.application.container_handle(),
            async move {
                if !self.ready {
                    return Response::empty(StatusCode::SERVICE_UNAVAILABLE);
                }
                let previous_exceptions = request
                    .exceptions
                    .replace(self.application.shared.exceptions.clone());
                let previous_views = request.parts_mut().extensions.insert(
                    self.views
                        .as_ref()
                        .expect("ready kernel has a view factory")
                        .clone(),
                );
                let head = request.method() == Method::HEAD;
                request.set_route_parameters(Vec::new());
                let attributes = request.attributes().clone();
                let mut response = crate::http::request_context::scope(
                    self.application.container_handle(),
                    attributes,
                    Next::new(
                        &self.middleware.global,
                        self.routes.as_ref().expect("ready kernel has routes"),
                    )
                    .run(request),
                )
                .await;
                request.exceptions = previous_exceptions;
                let extensions = &mut request.parts_mut().extensions;
                if let Some(views) = previous_views {
                    extensions.insert(views);
                } else {
                    extensions.remove::<Arc<Factory>>();
                }
                if head {
                    let body = response.take_body();
                    if !response.status().is_informational()
                        && !matches!(
                            response.status(),
                            StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED
                        )
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
            },
        ))
    }

    fn terminate<'a>(
        &'a self,
        _request: &'a RequestParts,
        _response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(async {})
    }
}
