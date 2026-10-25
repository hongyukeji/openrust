//! [`HttpPlugin`] and the route registration hook.

use std::convert::Infallible;
use std::future::Future;

use crate::core::{App, AppBuilder, AppState, Error, Plugin, Result};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::Route;
use tower::{Layer, Service};

use crate::http::client_ip::validate_trusted_proxy_ranges;
use crate::http::health;
use crate::http::middleware;
use crate::http::routes::Routes;
use crate::http::serve::ServeCommand;

type RouteBuilder = Box<dyn Fn(Routes) -> Routes + Send + Sync>;
type RouterLayerBuilder = Box<dyn Fn(Router) -> Router + Send + Sync>;

#[derive(Default)]
struct RouterLayerRegistry {
    builders: Vec<RouterLayerBuilder>,
}

/// Route builders collected during registration, applied in order when the router is built.
#[derive(Default)]
pub struct RouteRegistry {
    builders: Vec<RouteBuilder>,
}

impl RouteRegistry {
    /// Number of registered route builders.
    pub fn len(&self) -> usize {
        self.builders.len()
    }

    /// `true` when nothing registered routes.
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
    }
}

/// Adds routes and Tower layers to [`AppBuilder`].
pub trait AppBuilderHttpExt {
    /// Contributes routes. Call it from [`Application::build`](crate::core::Application::build)
    /// or from a plugin's `build`; builders run in registration order.
    fn routes<F>(&mut self, builder: F) -> &mut Self
    where
        F: Fn(Routes) -> Routes + Send + Sync + 'static;

    /// Adds a Tower layer around all routes after route registration.
    ///
    /// Capability plugins use this to install request context middleware that must run outside
    /// route-level layers. The default HTTP response/error middleware remains outside this layer.
    fn http_layer<L>(&mut self, layer: L) -> &mut Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future:
            Future<Output = Result<Response, Infallible>> + Send + 'static;
}

impl AppBuilderHttpExt for AppBuilder {
    fn routes<F>(&mut self, builder: F) -> &mut Self
    where
        F: Fn(Routes) -> Routes + Send + Sync + 'static,
    {
        self.extension_mut::<RouteRegistry>()
            .builders
            .push(Box::new(builder));
        self
    }

    fn http_layer<L>(&mut self, layer: L) -> &mut Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future:
            Future<Output = Result<Response, Infallible>> + Send + 'static,
    {
        self.extension_mut::<RouterLayerRegistry>()
            .builders
            .push(Box::new(move |router| router.layer(layer.clone())));
        self
    }
}

/// Installs the HTTP capability: `serve` command, default middleware and `GET /health`.
///
/// Applications install this explicitly with `app.plugin(HttpPlugin)`.
pub struct HttpPlugin;

impl Plugin for HttpPlugin {
    fn name(&self) -> &'static str {
        "http"
    }

    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.command(ServeCommand).routes(health::routes);
        Ok(())
    }
}

/// Builds the final [`Router`] from every registered route builder, wrapped in the default stack.
pub fn build_router(app: &App) -> Result<Router> {
    validate_trusted_proxy_ranges(&app.config().server.trusted_proxy_ranges)?;
    let mut routes = Routes::new();
    if let Some(registry) = app.extension::<RouteRegistry>() {
        for builder in &registry.builders {
            routes = builder(routes);
        }
    }
    let mut stateful = routes
        .into_router()
        .fallback(|| async { Error::NotFound("route not found".into()) })
        .method_not_allowed_fallback(|| async {
            Error::custom_with_status(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "method not allowed",
            )
        })
        .with_state(app.state().clone());
    if let Some(layers) = app.extension::<RouterLayerRegistry>() {
        for build_layer in &layers.builders {
            stateful = build_layer(stateful);
        }
    }
    let app_router = middleware::apply(stateful, &app.config().server, &app.config().api)?;
    Ok(app_router.layer(axum::middleware::from_fn_with_state(
        app.state().clone(),
        expose_app_state,
    )))
}

async fn expose_app_state(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(state);
    next.run(request).await
}
