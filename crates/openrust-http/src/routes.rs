//! Routing convention over [`axum::Router`].

use std::convert::Infallible;

use axum::Router;
use axum::extract::Request;
use axum::handler::Handler;
use axum::response::IntoResponse;
use axum::routing::{self, MethodRouter, Route};
use openrust_core::AppState;
use tower::{Layer, Service};

use crate::resource::{Resource, ResourceActions};

/// Collects routes for an application. Converted into an [`axum::Router`] when the server starts.
///
/// Every method consumes and returns `Self` so route tables read top-down:
///
/// ```
/// use openrust_http::Routes;
///
/// async fn index() -> &'static str { "posts" }
/// async fn show() -> &'static str { "post" }
///
/// fn register(routes: Routes) -> Routes {
///     routes.group("/api", |api| api.get("/posts", index).get("/posts/{id}", show))
/// }
/// ```
#[derive(Default)]
pub struct Routes {
    router: Router<AppState>,
}

impl Routes {
    /// Empty route table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a route with an explicit [`MethodRouter`].
    #[must_use]
    pub fn route(mut self, path: &str, method_router: MethodRouter<AppState>) -> Self {
        self.router = self.router.route(path, method_router);
        self
    }

    /// `GET` route.
    #[must_use]
    pub fn get<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::get(handler))
    }

    /// `POST` route.
    #[must_use]
    pub fn post<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::post(handler))
    }

    /// `PUT` route.
    #[must_use]
    pub fn put<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::put(handler))
    }

    /// `PATCH` route.
    #[must_use]
    pub fn patch<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::patch(handler))
    }

    /// `DELETE` route.
    #[must_use]
    pub fn delete<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::delete(handler))
    }

    /// Registers the actions explicitly supplied by a resource controller.
    ///
    /// # Panics
    ///
    /// Axum panics during registration if a route conflicts with an existing route
    /// (the same path and method, or different parameter names at the same position).
    /// Member routes always use the parameter name `id`.
    #[must_use]
    pub fn resource<R: Resource>(self, path: &str) -> Self {
        self.resource_with::<R>(path, |actions| actions)
    }

    /// Registers a resource after applying `f` to its actions.
    ///
    /// # Panics
    ///
    /// Axum panics during registration if a route conflicts with an existing route
    /// (the same path and method, or different parameter names at the same position).
    /// Member routes always use the parameter name `id`.
    #[must_use]
    pub fn resource_with<R: Resource>(
        self,
        path: &str,
        f: impl FnOnce(ResourceActions) -> ResourceActions,
    ) -> Self {
        f(R::actions(ResourceActions::new())).mount(self, path)
    }

    /// Nests the routes built by `f` under `prefix`. An empty or `/` prefix merges instead.
    #[must_use]
    pub fn group(mut self, prefix: &str, f: impl FnOnce(Routes) -> Routes) -> Self {
        let inner = f(Routes::new()).router;
        self.router = if prefix.is_empty() || prefix == "/" {
            self.router.merge(inner)
        } else {
            self.router.nest(prefix, inner)
        };
        self
    }

    /// Merges another route table at the same level.
    #[must_use]
    pub fn merge(mut self, other: Routes) -> Self {
        self.router = self.router.merge(other.router);
        self
    }

    /// Applies a `tower` layer to every route registered so far.
    #[must_use]
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.router = self.router.layer(layer);
        self
    }

    /// Hands the collected routes to axum.
    pub fn into_router(self) -> Router<AppState> {
        self.router
    }
}
