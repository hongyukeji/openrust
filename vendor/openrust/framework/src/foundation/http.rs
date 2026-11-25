//! HTTP request lifecycle for a bootstrapped application.

pub mod events;
mod form_request;
mod health_check;
pub mod middleware;
mod prerender;
mod server;
mod termination;

use termination::Termination;
pub(crate) use termination::terminable;
use tokio_util::task::TaskTracker;

pub use form_request::{FormRequest, FormRequestDefaults, FormRequestError, ValidatedFormRequest};
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
    browser_logs: Option<Arc<crate::log::browser::Control>>,
    ready: bool,
    termination_tasks: TaskTracker,
    routes: Option<Arc<crate::routing::Router>>,
}

impl Kernel {
    async fn render_exception(
        &self,
        request: &mut Request,
        error: crate::http::HttpError,
    ) -> Response {
        match self.application.exception_handler() {
            Ok(handler) => {
                let error = Arc::new(error);
                super::exceptions::view_exception::scope(
                    error.clone(),
                    handler.handle_http(request, error),
                )
                .await
            }
            Err(source) => crate::http::HttpError::from(source).bare_response(),
        }
    }

    pub fn new(application: Application) -> Self {
        let middleware = application.configured_middleware().clone();
        Self {
            views: None,
            browser_logs: None,
            application,
            middleware,
            ready: false,
            termination_tasks: TaskTracker::new(),
            routes: None,
        }
    }

    async fn request_received(
        &self,
        event: events::RequestReceived,
    ) -> Result<(), crate::http::HttpError> {
        use crate::contracts::events::Dispatcher as _;
        let dispatcher = self
            .application
            .container()
            .make::<crate::events::Dispatcher>()?;
        dispatcher
            .dispatch(Arc::new(event), &mut crate::events::EventContext::new())
            .await?;
        Ok(())
    }

    async fn request_handled(
        &self,
        request: &mut Request,
        mut response: Response,
        started_at: std::time::Instant,
        request_started_at: i64,
        head: bool,
    ) -> Response {
        self.prepare_response(request, &mut response, head);
        let event = Arc::new(events::RequestHandled::new(
            request,
            &response,
            request_started_at,
            started_at.elapsed(),
        ));
        let dispatched = match self
            .application
            .container()
            .make::<crate::events::Dispatcher>()
        {
            Ok(dispatcher) => {
                use crate::contracts::events::Dispatcher as _;
                dispatcher
                    .dispatch(event, &mut crate::events::EventContext::new())
                    .await
                    .map_err(Into::into)
            }
            Err(source) => Err(crate::http::HttpError::Internal {
                source: Box::new(source),
            }),
        };
        if let Err(error) = dispatched {
            response = self.render_exception(request, error).await;
            self.prepare_response(request, &mut response, head);
        }
        if let Some(control) = &self.browser_logs {
            crate::log::browser::append(
                &self.application.container_handle(),
                control,
                &mut response,
                head,
            );
        }
        response
    }

    fn prepare_response(&self, request: &Request, response: &mut Response, head: bool) {
        response.prepare_binary_file(request.method(), request.headers());
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
                self.middleware
                    .set_locale
                    .configure(&self.application)
                    .await
                    .map_err(|source| BootstrapError::LifecycleFailed {
                        source: Box::new(source),
                    })?;
                let config = self.application.config()?;
                if self.middleware.trusted_proxies.is_none() {
                    self.middleware.trusted_proxies =
                        Some(crate::http::ProxyPolicy::from_config(config.as_ref())?);
                }
                if let Some(hosts) = &mut self.middleware.trusted_hosts {
                    let environment = self.application.container().get::<str>("env")?;
                    if matches!(environment.as_ref(), "local" | "testing") {
                        self.middleware.trusted_hosts = None;
                    } else {
                        hosts.configure(config.as_ref())?;
                    }
                }
                self.middleware.configure_web(config.as_ref())?;
                let cors = crate::http::middleware::HandleCors::from_config(
                    config.get("cors"),
                    &self.middleware.cors_skip_callbacks,
                )?;
                let browser_logs = self
                    .application
                    .container()
                    .make::<crate::log::browser::Control>()?;
                if config
                    .get("logging.channels")
                    .and_then(|v| v.as_object().cloned())
                    .is_some_and(|channels| {
                        channels.values().any(|channel| {
                            channel.get("handler").and_then(serde_json::Value::as_str)
                                == Some("browser_console")
                        })
                    })
                {
                    browser_logs
                        .0
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                self.browser_logs = Some(browser_logs);
                self.views = Some(
                    self.application
                        .container()
                        .get::<Factory>(crate::container::ServiceKey::typed::<Factory>())?,
                );
                self.application.maintenance_mode()?;
                if self.routes.is_none() {
                    let mut routes = self
                        .application
                        .shared
                        .routes
                        .lock()
                        .expect("application routes lock poisoned")
                        .take()
                        .expect("boot assembled the application routes");
                    if let Err(error) = routes.configure_middleware(
                        &self.middleware.registry,
                        &self.middleware.set_locale,
                    ) {
                        *self
                            .application
                            .shared
                            .routes
                            .lock()
                            .expect("application routes lock poisoned") = Some(routes);
                        return Err(error.into());
                    }
                    self.routes = Some(Arc::new(routes));
                }
                self.application.container().instance_named(
                    "router",
                    self.routes
                        .as_ref()
                        .expect("boot assembled the application routes")
                        .clone(),
                )?;
                self.application.container().alias(
                    "router",
                    crate::container::ServiceKey::typed::<crate::routing::Router>(),
                )?;
                let maintenance_index = self.middleware.prepended + usize::from(cors.is_some());
                if let Some(cors) = cors {
                    self.middleware
                        .global
                        .insert(self.middleware.prepended, Arc::new(cors));
                }
                self.middleware.global.insert(
                    maintenance_index,
                    Arc::new(
                        middleware::PreventRequestsDuringMaintenance::for_application(
                            Arc::downgrade(&self.application.container_handle()),
                            std::mem::take(&mut self.middleware.maintenance_exceptions),
                        ),
                    ),
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
                if self
                    .application
                    .shared
                    .prefer_json
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    self.middleware
                        .prepend(Arc::new(crate::http::middleware::PrefersJsonResponses));
                }
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
        use super::observability::{self, Operation, Outcome};
        let observation = observability::begin(
            self.application.container(),
            Operation::Http {
                method: request.method(),
                headers: request.headers(),
            },
        );
        let lifecycle =
            crate::support::facades::scope(self.application.container_handle(), async move {
                if !self.ready {
                    return Response::empty(StatusCode::SERVICE_UNAVAILABLE);
                }
                let started_at = std::time::Instant::now();
                let request_started_at = jiff::Timestamp::now().as_millisecond();
                request
                    .parts_mut()
                    .extensions
                    .remove::<crate::http::middleware::LocaleSelection>();
                request
                    .parts_mut()
                    .extensions
                    .remove::<crate::http::middleware::PrivateLocaleResponse>();
                request.matched_route = false;
                request
                    .parts_mut()
                    .extensions
                    .insert(self.middleware.method_override.clone());
                request.route_name = None;
                request
                    .parts_mut()
                    .extensions
                    .remove::<crate::routing::RouteMetadata>();
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
                    return crate::http::request_context::scope(
                        self.application.container_handle(),
                        request.uri().clone(),
                        request.headers().get(::http::header::HOST).cloned(),
                        request.get_base_url().to_owned(),
                        request.attributes().clone(),
                        request.ips().to_vec(),
                        async {
                            let response = self.render_exception(request, error).await;
                            let head = request.method() == Method::HEAD;
                            let response = self
                                .request_handled(
                                    request,
                                    response,
                                    started_at,
                                    request_started_at,
                                    head,
                                )
                                .await;
                            request.api_prefix = previous_api_prefix;
                            response
                        },
                    )
                    .await;
                }
                let previous_exceptions = request
                    .exceptions
                    .replace(Arc::downgrade(&self.application.container_handle()));
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
                    request.get_base_url().to_owned(),
                    attributes,
                    request.ips().to_vec(),
                    crate::view::middleware::request_scope(
                        crate::cookie::middleware::request_scope(async {
                            let response = match self
                                .request_received(events::RequestReceived::new(request))
                                .await
                            {
                                Ok(()) => {
                                    Next::new(
                                        &self.middleware.global,
                                        self.routes.as_deref().expect("ready kernel has routes"),
                                    )
                                    .run(request)
                                    .await
                                }
                                Err(error) => self.render_exception(request, error).await,
                            };
                            self.request_handled(
                                request,
                                response,
                                started_at,
                                request_started_at,
                                head,
                            )
                            .await
                        }),
                    ),
                ));
                let request_lifecycle = crate::log::context::Origin::scope(
                    self.application.container_handle(),
                    log_origin,
                    request_lifecycle,
                );
                let response = deferred.scope(request_lifecycle).await;
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
                response
            });
        Box::pin(observation.run(lifecycle, |response| Outcome::Http(response.status())))
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
