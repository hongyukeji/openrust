//! [`HttpPlugin`] and the route registration hook.

use axum::Router;
use axum::http::StatusCode;
use openrust_core::{App, AppBuilder, Error, Plugin, Result};

use crate::health;
use crate::middleware;
use crate::routes::Routes;
use crate::serve::ServeCommand;

type RouteBuilder = Box<dyn Fn(Routes) -> Routes + Send + Sync>;

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

/// Adds [`routes`](AppBuilderHttpExt::routes) to [`AppBuilder`].
pub trait AppBuilderHttpExt {
    /// Contributes routes. Call it from [`Application::build`](openrust_core::Application::build)
    /// or from a plugin's `build`; builders run in registration order.
    fn routes<F>(&mut self, builder: F) -> &mut Self
    where
        F: Fn(Routes) -> Routes + Send + Sync + 'static;
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
    let mut routes = Routes::new();
    if let Some(registry) = app.extension::<RouteRegistry>() {
        for builder in &registry.builders {
            routes = builder(routes);
        }
    }
    let stateful = routes
        .into_router()
        .fallback(|| async { Error::NotFound("route not found".into()) })
        .method_not_allowed_fallback(|| async {
            Error::custom(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "method not allowed",
            )
        })
        .with_state(app.state().clone());
    middleware::apply(stateful, &app.config().server)
}
