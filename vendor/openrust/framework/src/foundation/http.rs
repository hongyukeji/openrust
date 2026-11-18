//! HTTP request lifecycle for a bootstrapped application.

mod form_request;
mod health_check;
pub mod middleware;
mod prerender;
mod server;
mod termination;

use termination::Termination;
pub(crate) use termination::terminable;
use tokio_util::task::TaskTracker;

pub use form_request::{FormRequest, FormRequestError, ValidatedFormRequest};
pub use health_check::HealthCheck;

use std::sync::Arc;

use axum::body::HttpBody as _;

use super::{Application, BootstrapError, configuration::Middleware};
use crate::{
    concurrency::DeferredCallbacks,
    contracts::{container::Container as _, foundation::Application as _, http},
    http::{
        HttpFuture, Method, Next, Request, RequestParts, Response, ResponseParts, StatusCode,
        middleware::{ValidatePathEncoding, ValidatePostSize},
    },
    view::Factory,
};

/// Dispatches requests and coordinates middleware and error rendering.
pub struct Kernel {
    application: Application,
    middleware: Middleware,
    views: Option<Arc<Factory>>,
    ready: bool,
    termination_tasks: TaskTracker,
    routes: Option<crate::routing::Router>,
}

impl Kernel {
    pub fn new(application: Application) -> Self {
        let middleware = application.configured_middleware().clone();
        Self {
            views: None,
            application,
            middleware,
            ready: false,
            termination_tasks: TaskTracker::new(),
            routes: None,
        }
    }

    async fn prepare_request_origin(
        &self,
        request: &mut Request,
    ) -> Result<(), crate::http::HttpError> {
        request.proxy = Default::default();
        let hosts = if let Some(policy) = &self.middleware.trusted_hosts {
            let snapshot = policy.snapshot().await?;
            snapshot.check_incoming(request)?;
            Some(snapshot)
        } else {
            None
        };
        self.middleware
            .trusted_proxies
            .as_ref()
            .expect("ready kernel has a proxy policy")
            .apply(request)?;
        if let Some(hosts) = hosts {
            hosts.check_effective(request)?;
        }
        Ok(())
    }

    fn bootstrap_http(
        &mut self,
        configuration_ready: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> HttpFuture<'_, Result<(), BootstrapError>> {
        Box::pin(async move {
            if !self.ready {
                self.application
                    .boot_with_configuration_signal(None, configuration_ready)
                    .await?;
                let config = self
                    .application
                    .config()
                    .map_err(|source| BootstrapError::ContainerFailed { source })?;
                if self.middleware.trusted_proxies.is_none() {
                    self.middleware.trusted_proxies = Some(
                        crate::http::ProxyPolicy::from_config(config.as_ref())
                            .map_err(|source| BootstrapError::ConfigurationFailed { source })?,
                    );
                }
                if let Some(hosts) = &mut self.middleware.trusted_hosts {
                    let environment = self
                        .application
                        .container()
                        .get::<str>("env")
                        .map_err(|source| BootstrapError::ContainerFailed { source })?;
                    if matches!(environment.as_ref(), "local" | "testing") {
                        self.middleware.trusted_hosts = None;
                    } else {
                        hosts
                            .configure(config.as_ref())
                            .map_err(|source| BootstrapError::ConfigurationFailed { source })?;
                    }
                }
                self.middleware
                    .configure_web(config.as_ref())
                    .map_err(|source| BootstrapError::ConfigurationFailed { source })?;
                let cors = crate::http::middleware::HandleCors::from_config(
                    config.get("cors"),
                    &self.middleware.cors_skip_callbacks,
                )
                .map_err(|source| BootstrapError::ConfigurationFailed { source })?;
                self.views = Some(
                    self.application
                        .container()
                        .get::<Factory>(crate::container::ServiceKey::typed::<Factory>())
                        .map_err(|source| BootstrapError::ContainerFailed { source })?,
                );
                let maintenance = self
                    .application
                    .maintenanceMode()
                    .map_err(|source| BootstrapError::ContainerFailed { source })?;
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
                let maintenance_index = self.middleware.prepended + usize::from(cors.is_some());
                if let Some(cors) = cors {
                    self.middleware
                        .global
                        .insert(self.middleware.prepended, Arc::new(cors));
                }
                self.middleware.global.insert(
                    maintenance_index,
                    Arc::new(middleware::PreventRequestsDuringMaintenance::new(
                        maintenance,
                        std::mem::take(&mut self.middleware.maintenance_exceptions),
                    )),
                );
                self.middleware
                    .global
                    .insert(maintenance_index + 1, Arc::new(ValidatePostSize));
                self.middleware.global.insert(
                    maintenance_index + 2,
                    Arc::new(std::mem::take(&mut self.middleware.trim_strings)),
                );
                self.middleware.global.insert(
                    maintenance_index + 3,
                    Arc::new(std::mem::take(&mut self.middleware.empty_strings_to_null)),
                );
                self.ready = true;
            }
            Ok(())
        })
    }
}

impl http::Kernel for Kernel {
    fn bootstrap(&mut self) -> HttpFuture<'_, Result<(), BootstrapError>> {
        self.bootstrap_http(None)
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
                let previous_api_prefix = request.api_prefix.replace(
                    self.application
                        .shared
                        .api_prefix
                        .get()
                        .expect("ready kernel has an API prefix")
                        .clone(),
                );
                let origin = match ValidatePathEncoding::validate(request.parts().uri.path()) {
                    Ok(()) => self.prepare_request_origin(request).await,
                    Err(error) => Err(error),
                };
                if let Err(error) = origin {
                    let response = self
                        .application
                        .shared
                        .exceptions
                        .render_http(request, &error)
                        .await;
                    request.api_prefix = previous_api_prefix;
                    return response;
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
                crate::auth::reset_request(request);
                #[cfg(feature = "sanctum")]
                crate::auth::AuthManager::apply_test_identity(request);
                request.set_route_parameters(Vec::new());
                request.route_terminators.clear();
                request.matched_route = false;
                let attributes = request.attributes().clone();
                let deferred = DeferredCallbacks::default();
                let log_origin = crate::log::context::Origin::enabled(self.application.container())
                    .then(|| {
                        let default_guard = self
                            .application
                            .config()
                            .ok()
                            .and_then(|config| config.get("auth.defaults.guard"))
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .unwrap_or_else(|| "web".into());
                        crate::log::context::Origin::http(request, default_guard)
                    });
                let request_lifecycle = Box::pin(crate::http::request_context::scope(
                    self.application.container_handle(),
                    request.uri().clone(),
                    request.headers().get(::http::header::HOST).cloned(),
                    request.getBaseUrl().to_owned(),
                    attributes,
                    crate::view::middleware::request_scope(
                        crate::cookie::middleware::request_scope(
                            Next::new(
                                &self.middleware.global,
                                self.routes.as_ref().expect("ready kernel has routes"),
                            )
                            .run(request),
                        ),
                    ),
                ));
                let request_lifecycle = crate::log::context::Origin::scope(
                    self.application.container_handle(),
                    log_origin,
                    request_lifecycle,
                );
                let mut response = deferred.scope(request_lifecycle).await;
                response.prepare_binary_file(request.method(), request.headers());
                request.exceptions = previous_exceptions;
                request.api_prefix = previous_api_prefix;
                let mut middleware = std::mem::take(&mut request.route_terminators);
                middleware.extend(
                    self.middleware
                        .global
                        .iter()
                        .filter(|middleware| middleware.terminable().is_some())
                        .cloned(),
                );
                let extensions = &mut request.parts_mut().extensions;
                if deferred.is_empty() {
                    extensions.remove::<DeferredCallbacks>();
                } else {
                    extensions.insert(deferred);
                }
                if middleware.is_empty() {
                    extensions.remove::<Termination>();
                } else {
                    extensions.insert(Termination::new(middleware));
                }
                if let Some(views) = previous_views {
                    extensions.insert(views);
                } else {
                    extensions.remove::<Arc<Factory>>();
                }
                if !Response::allows_body(response.status()) {
                    drop(response.take_body());
                    for name in [
                        ::http::header::CONTENT_TYPE,
                        ::http::header::CONTENT_LENGTH,
                        ::http::header::TRANSFER_ENCODING,
                    ] {
                        response.parts_mut().headers.remove(name);
                    }
                } else if head {
                    let body = response.take_body();
                    if let Some(length) = body.size_hint().exact() {
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
        request: &'a RequestParts,
        response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(async move {
            let termination = request.extensions.get::<Termination>();
            let deferred = request.extensions.get::<DeferredCallbacks>();
            if termination.is_none() && deferred.is_none() {
                return;
            }
            crate::support::facades::scope(self.application.container_handle(), async move {
                if let Some(termination) = termination {
                    for middleware in termination.take() {
                        if let Some(terminable) = middleware.terminable() {
                            terminable.terminate(request, response).await;
                        }
                    }
                }
                if let Some(deferred) = deferred {
                    deferred
                        .invoke(&self.application, response.status.as_u16() < 400)
                        .await;
                }
            })
            .await;
        })
    }
}
