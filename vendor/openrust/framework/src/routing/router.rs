use std::{
    any::type_name_of_val,
    collections::{HashMap, HashSet},
    panic::Location,
    path::Path,
    sync::Arc,
};

use percent_encoding::percent_decode_str;
use serde::Serialize;

use crate::{
    contracts::routing::{Handler, Registrar, UrlRoutable},
    http::{ApiResponse, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    view::View,
};

use super::{
    BindingContext, MiddlewareEntry, Route, RouteGroup, RoutingError,
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

impl<F, T> IntoRouteHandler<(ApiResponse<T>,)> for F
where
    F: Fn() -> ApiResponse<T> + Send + Sync + 'static,
    T: Serialize,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(ApiResponseHandler(self))
    }
}

struct ApiResponseHandler<F>(F);

impl<F, T> Handler for ApiResponseHandler<F>
where
    F: Fn() -> ApiResponse<T> + Send + Sync,
    T: Serialize,
{
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)().into_response() })
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
    has_domains: bool,
}

/// Metadata for the methods that still dispatch to a registered route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RouteInfo {
    pub domain: Option<String>,
    pub method: String,
    pub uri: String,
    pub name: Option<String>,
    pub action: String,
    pub path: String,
    #[serde(skip)]
    pub vendor: bool,
    pub middleware: Vec<String>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Router API")]
    pub fn getRoutes(&self) -> Vec<RouteInfo> {
        let mut methods_by_route = vec![Vec::new(); self.routes.len()];
        for (method, indices) in &self.by_method {
            for &index in indices {
                methods_by_route[index].push(method.as_str());
            }
        }

        self.routes
            .iter()
            .zip(methods_by_route)
            .filter(|(_, methods)| !methods.is_empty())
            .map(|(route, mut methods)| {
                methods.sort_unstable_by_key(|method| {
                    (
                        VERBS
                            .iter()
                            .position(|verb| verb == method)
                            .unwrap_or(VERBS.len()),
                        *method,
                    )
                });
                let uri = route
                    .uri
                    .strip_prefix('/')
                    .filter(|uri| !uri.is_empty())
                    .unwrap_or(&route.uri);
                let file = route.definition.file().replace('\\', "/");
                let vendor = file.split('/').any(|component| component == "vendor");
                RouteInfo {
                    domain: route.domain.clone(),
                    method: methods.join("|"),
                    uri: uri.to_owned(),
                    name: route.getName().map(str::to_owned),
                    action: route.action.to_owned(),
                    path: format!("{file}:{}", route.definition.line()),
                    vendor,
                    middleware: route
                        .middleware
                        .iter()
                        .map(|middleware| match middleware {
                            MiddlewareEntry::Named(name) => name.clone(),
                            MiddlewareEntry::Instance(_) => "Closure".to_owned(),
                        })
                        .collect(),
                }
            })
            .collect()
    }

    #[track_caller]
    pub fn get<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::GET, Method::HEAD], uri, handler)
    }

    #[track_caller]
    pub fn r#match<Arguments>(
        &mut self,
        methods: &[Method],
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.register_with(methods, uri, handler, Location::caller())
    }

    pub(crate) fn register_with<Arguments>(
        &mut self,
        methods: &[Method],
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
        definition: &'static Location<'static>,
    ) -> Result<&mut Route, RoutingError> {
        let action = type_name_of_val(&handler);
        let route = self.add_route(methods, uri, handler.into_route_handler())?;
        route.definition = definition;
        if !action.contains("{{closure}}") {
            route.action = action;
        }
        Ok(route)
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

    pub(crate) fn mount_public_disk(
        &mut self,
        prefix: &str,
        directory: &Path,
    ) -> Result<(), RoutingError> {
        self.static_mounts
            .push(StaticMount::public_disk(prefix, directory)?);
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
            route.resolved_middleware = Some(registry.resolve_stack(
                &route.middleware,
                &route.excluded_middleware,
                &groups,
            )?);
        }
        Ok(())
    }

    fn matched(&self, method: &Method, host: &str, path: &str) -> Option<&Route> {
        self.by_method
            .get(method)?
            .iter()
            .map(|&index| &self.routes[index])
            .find(|route| route.matches(host, path))
    }

    pub async fn dispatch(&self, request: &mut Request) -> Result<Response, HttpError> {
        request.set_route_parameters(Vec::new());
        request.route_terminators.clear();
        request.matched_route = false;
        let path = request.uri().path().trim_end_matches('/');
        let path = percent_decode_str(if path.is_empty() { "/" } else { path })
            .decode_utf8()
            .map_err(|_| HttpError::InvalidRequest)?;
        let authority = (self.has_domains && request.uri().host().is_none())
            .then(|| request.headers().get(::http::header::HOST))
            .flatten()
            .and_then(|header| header.to_str().ok())
            .and_then(|host| host.parse::<::http::uri::Authority>().ok());
        let host = request
            .uri()
            .host()
            .or_else(|| authority.as_ref().map(::http::uri::Authority::host))
            .unwrap_or_default();
        if let Some(route) = self.matched(request.method(), host, &path) {
            let parameters = route.parameters(host, &path);
            request.matched_route = true;
            let plan = route.binding_plan(&self.bindings, &parameters);
            request.set_route_parameters(parameters);
            request.binding_plan = plan;
            let resolved;
            let middleware = if let Some(stack) = &route.resolved_middleware {
                stack
            } else {
                resolved = MiddlewareRegistry::default()
                    .resolve_stack(
                        &route.middleware,
                        &route.excluded_middleware,
                        &HashMap::new(),
                    )
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                &resolved
            };
            request.route_terminators.extend(
                middleware
                    .iter()
                    .filter(|middleware| middleware.terminable().is_some())
                    .cloned(),
            );
            return Ok(Next::new(middleware, route.handler.as_ref())
                .run(request)
                .await);
        }

        let allowed: Vec<_> = VERBS
            .iter()
            .map(|verb| Method::from_bytes(verb.as_bytes()).expect("valid HTTP method"))
            .filter(|method| self.matched(method, host, &path).is_some())
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
    #[track_caller]
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
        let domain = self
            .groups
            .iter()
            .rev()
            .find_map(|group| group.domain.clone());
        let mut constraints = self.patterns.clone();
        for group in &self.groups {
            constraints.extend(group.constraints.clone());
        }
        let mut route = Route::new(uri, domain, handler, middleware, constraints)?;
        route.name = self
            .groups
            .iter()
            .map(|group| group.name.as_str())
            .collect();
        route.excluded_middleware = self
            .groups
            .iter()
            .flat_map(|group| group.excluded_middleware.iter().cloned())
            .collect();
        self.has_domains |= route.domain.is_some();
        let index = self.routes.len();
        for method in methods.iter().collect::<HashSet<_>>() {
            let routes = self.by_method.entry(method.clone()).or_default();
            if let Some(slot) = routes.iter_mut().find(|&&mut entry| {
                let previous = &self.routes[entry];
                previous.uri == route.uri && previous.domain == route.domain
            }) {
                *slot = index;
            } else {
                routes.push(index);
            }
        }
        self.routes.push(route);
        Ok(&mut self.routes[index])
    }
}
