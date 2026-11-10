use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use percent_encoding::percent_decode_str;

use crate::{
    contracts::routing::{Handler, Registrar, UrlRoutable},
    http::{HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    view::View,
};

use super::{
    BindingContext, Route, RouteGroup, RoutingError,
    binding::{self, Resolver},
    middleware::MiddlewareRegistry,
    route_pattern::constraint,
    static_mount::StaticMount,
};

const VERBS: [&str; 8] = [
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY",
];

/// Converts a route callback into a handler for the HTTP pipeline.
pub trait IntoRouteHandler<Arguments> {
    fn into_route_handler(self) -> Arc<dyn Handler>;
}

impl<H: Handler + 'static> IntoRouteHandler<(Request,)> for H {
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(self)
    }
}

impl<F> IntoRouteHandler<()> for F
where
    F: Fn() -> View + Send + Sync + 'static,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(ViewHandler(self))
    }
}

struct ViewHandler<F>(F);

impl<F: Fn() -> View + Send + Sync> Handler for ViewHandler<F> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        (self.0)().render(request)
    }
}

/// Registers routes and dispatches requests to their handlers.
#[derive(Default)]
pub struct Router {
    routes: Vec<Route>,
    by_method: HashMap<Method, Vec<usize>>,
    groups: Vec<RouteGroup>,
    patterns: HashMap<String, String>,
    bindings: HashMap<String, Resolver>,
    static_mounts: Vec<StaticMount>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.add_route(
            &[Method::GET, Method::HEAD],
            uri,
            handler.into_route_handler(),
        )
    }

    pub fn group(
        &mut self,
        group: RouteGroup,
        register: impl FnOnce(&mut Self) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        self.groups.push(group);
        let result = register(self);
        self.groups.pop();
        result
    }

    pub(crate) fn mount_static(
        &mut self,
        prefix: &str,
        directory: &Path,
    ) -> Result<(), RoutingError> {
        self.static_mounts
            .push(StaticMount::new(prefix, directory)?);
        Ok(())
    }

    pub(crate) fn resolve_static_directories(&mut self, base: &Path) {
        for mount in &mut self.static_mounts {
            mount.resolve_directory(base);
        }
    }

    pub fn bind<T, F>(&mut self, name: impl Into<String>, resolve: F) -> &mut Self
    where
        T: Send + Sync + 'static,
        F: for<'a> Fn(BindingContext<'a>) -> HttpFuture<'a, Result<Option<T>, HttpError>>
            + Send
            + Sync
            + 'static,
    {
        self.bindings
            .insert(name.into(), binding::resolver(resolve));
        self
    }

    pub fn model<T: UrlRoutable>(&mut self, name: impl Into<String>) -> &mut Self {
        self.bindings.insert(name.into(), binding::model::<T>());
        self
    }

    pub fn pattern(&mut self, name: &str, expression: &str) -> Result<&mut Self, RoutingError> {
        self.patterns
            .insert(name.to_owned(), constraint(name, expression)?);
        Ok(self)
    }

    pub(crate) fn configure_middleware(
        &mut self,
        registry: &MiddlewareRegistry,
    ) -> Result<(), RoutingError> {
        let groups = registry.resolve_groups()?;
        let active_routes: HashSet<_> = self.by_method.values().flatten().copied().collect();
        for index in active_routes {
            let route = &mut self.routes[index];
            route.resolved_middleware = Some(registry.resolve_stack(&route.middleware, &groups)?);
        }
        Ok(())
    }

    fn matched(&self, method: &Method, path: &str) -> Option<&Route> {
        self.by_method
            .get(method)?
            .iter()
            .map(|&index| &self.routes[index])
            .find(|route| route.matches(path))
    }

    pub async fn dispatch(&self, request: &mut Request) -> Result<Response, HttpError> {
        request.set_route_parameters(Vec::new());
        let path = request.uri().path().trim_end_matches('/');
        let path = percent_decode_str(if path.is_empty() { "/" } else { path })
            .decode_utf8()
            .map_err(|_| HttpError::InvalidRequest)?;
        if let Some(route) = self.matched(request.method(), &path) {
            let parameters = route.parameters(&path);
            let plan = route.binding_plan(&self.bindings, &parameters);
            request.set_route_parameters(parameters);
            request.binding_plan = plan;
            let resolved;
            let middleware = if let Some(stack) = &route.resolved_middleware {
                stack
            } else {
                resolved = MiddlewareRegistry::default()
                    .resolve_stack(&route.middleware, &HashMap::new())
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                &resolved
            };
            return Ok(Next::new(middleware, route.handler.as_ref())
                .run(request)
                .await);
        }

        let allowed: Vec<_> = VERBS
            .iter()
            .map(|verb| Method::from_bytes(verb.as_bytes()).expect("valid HTTP method"))
            .filter(|method| self.matched(method, &path).is_some())
            .collect();
        if allowed.is_empty() {
            if matches!(*request.method(), Method::GET | Method::HEAD)
                && !request.uri().path().ends_with('/')
            {
                for mount in &self.static_mounts {
                    if let Some(response) = mount
                        .serve(&path, request.method(), request.headers())
                        .await
                    {
                        return Ok(response);
                    }
                }
            }
            return Err(HttpError::NotFound);
        }
        if request.method() == Method::OPTIONS {
            return Ok(Response::empty(StatusCode::OK).with_allow(&allowed, ","));
        }
        Err(HttpError::MethodNotAllowed { allowed })
    }
}

impl Handler for Router {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(self.dispatch(request))
    }
}

impl Registrar for Router {
    fn add_route(
        &mut self,
        methods: &[Method],
        uri: &str,
        handler: Arc<dyn Handler>,
    ) -> Result<&mut Route, RoutingError> {
        if methods.is_empty() {
            return Err(RoutingError::MissingMethods);
        }
        let mut uri = self
            .groups
            .iter()
            .map(|group| group.prefix.as_str())
            .chain(std::iter::once(uri))
            .map(|part| part.trim_matches('/'))
            .filter(|part| !part.is_empty())
            .fold(String::new(), |mut uri, part| {
                uri.push('/');
                uri.push_str(part);
                uri
            });
        if uri.is_empty() {
            uri.push('/');
        }
        let middleware = self
            .groups
            .iter()
            .flat_map(|group| group.middleware.iter().cloned())
            .collect();
        let route = Route::new(uri, handler, middleware, self.patterns.clone())?;
        let index = self.routes.len();
        for method in methods.iter().collect::<HashSet<_>>() {
            let routes = self.by_method.entry(method.clone()).or_default();
            if let Some(slot) = routes
                .iter_mut()
                .find(|&&mut entry| self.routes[entry].uri == route.uri)
            {
                *slot = index;
            } else {
                routes.push(index);
            }
        }
        self.routes.push(route);
        Ok(&mut self.routes[index])
    }
}
