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
};

use super::{
    BindingContext, MiddlewareEntry, Route, RouteGroup, RoutingError,
    binding::{self, Resolver},
    middleware::MiddlewareRegistry,
    route_pattern::constraint,
    static_mount::StaticMount,
};

const VERBS: &[&str] = &[
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY",
];

struct RouteDispatcher<'a>(&'a dyn Handler);

impl Handler for RouteDispatcher<'_> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        if request.is_precognitive() {
            self.0.handle_precognitive(request)
        } else {
            self.0.handle(request)
        }
    }
}

/// Converts a route callback into a handler for the HTTP pipeline.
pub trait IntoRouteHandler<Arguments> {
    fn into_route_handler(self) -> Arc<dyn Handler>;
}

impl<H: Handler + 'static> IntoRouteHandler<(Request,)> for H {
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(self)
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
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async {
            request
                .with_container(async { (self.0)().into_response() })
                .await
        })
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
    route_cache: Option<super::route_cache::RouteCache>,
    locale_middleware: Vec<Arc<dyn crate::contracts::http::Middleware>>,
    group_middleware: HashMap<String, Vec<MiddlewareEntry>>,
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

    /// 在现有组末尾追加中间件，保留应用装配的Cookie、会话及优先级。
    pub fn push_middleware_to_group(
        &mut self,
        group: impl Into<String>,
        middleware: MiddlewareEntry,
    ) -> &mut Self {
        let entries = self.group_middleware.entry(group.into()).or_default();
        if !entries.iter().any(|entry| match (entry, &middleware) {
            (MiddlewareEntry::Named(a), MiddlewareEntry::Named(b)) => a == b,
            (MiddlewareEntry::Instance(a), MiddlewareEntry::Instance(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }) {
            entries.push(middleware);
        }
        self
    }

    fn middleware_registry(&self, registry: &MiddlewareRegistry) -> MiddlewareRegistry {
        let mut registry = registry.clone();
        for (group, entries) in &self.group_middleware {
            registry
                .groups
                .entry(group.clone())
                .or_default()
                .extend(entries.iter().cloned());
        }
        registry
    }

    pub(crate) fn compile_cache(&self) -> Option<super::route_cache::RouteCache> {
        (!self.by_method.is_empty())
            .then(|| super::route_cache::RouteCache::compile(&self.by_method, &self.routes))
    }

    pub(crate) fn load_cache(&mut self, path: &std::path::Path) -> bool {
        self.route_cache =
            super::route_cache::RouteCache::load(path, &self.by_method, &self.routes);
        self.route_cache.is_some()
    }

    pub(crate) fn url_routes(&self) -> super::url_generator::UrlRoutes {
        let active: HashSet<_> = self.by_method.values().flatten().copied().collect();
        let mut urls = super::url_generator::UrlRoutes::default();
        for (index, route) in self.routes.iter().enumerate() {
            if active.contains(&index)
                && let Some(name) = route.get_name()
            {
                urls.named
                    .entry(name.to_owned())
                    .or_insert_with(|| route.url_pattern());
            }
        }
        // the framework rebuilds the action lookup with domain routes first.
        for domain in [true, false] {
            for (index, route) in self.routes.iter().enumerate() {
                if route.domain.is_some() == domain
                    && active.contains(&index)
                    && let Some(action) = &route.controller_action
                {
                    urls.actions
                        .entry(action.clone())
                        .or_insert_with(|| route.url_pattern());
                }
            }
        }
        urls
    }

    pub fn get_routes(&self) -> Vec<RouteInfo> {
        self.routes_with_info().map(|(_, info)| info).collect()
    }

    pub(crate) fn resolved_route_info(
        &self,
        registry: &MiddlewareRegistry,
    ) -> Result<Vec<RouteInfo>, RoutingError> {
        let registry = self.middleware_registry(registry);
        let groups = registry.resolve_groups()?;
        self.routes_with_info()
            .map(|(route, mut info)| {
                let stack = registry.resolve_stack(
                    &route.middleware,
                    &route.excluded_middleware,
                    &groups,
                )?;
                info.middleware = registry.describe_stack(&stack);
                Ok(info)
            })
            .collect()
    }

    fn routes_with_info(&self) -> impl Iterator<Item = (&Route, RouteInfo)> {
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
                (
                    route,
                    RouteInfo {
                        domain: route.domain.clone(),
                        method: methods.join("|"),
                        uri: uri.to_owned(),
                        name: route.get_name().map(str::to_owned),
                        action: route
                            .controller_action
                            .as_deref()
                            .unwrap_or(route.action)
                            .to_owned(),
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
                    },
                )
            })
    }

    #[track_caller]
    pub fn get<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::GET, Method::HEAD], uri, handler)
    }

    /// 注册仅接受 POST 的路由，并返回可配置的路由对象。
    #[track_caller]
    pub fn post<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::POST], uri, handler)
    }

    /// 注册仅接受 PUT 的路由，并返回可配置的路由对象。
    #[track_caller]
    pub fn put<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::PUT], uri, handler)
    }

    /// 注册仅接受 PATCH 的路由，并返回可配置的路由对象。
    #[track_caller]
    pub fn patch<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::PATCH], uri, handler)
    }

    /// 注册仅接受 DELETE 的路由，并返回可配置的路由对象。
    #[track_caller]
    pub fn delete<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::DELETE], uri, handler)
    }

    /// 注册仅接受 OPTIONS 的路由，并返回可配置的路由对象。
    #[track_caller]
    pub fn options<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::OPTIONS], uri, handler)
    }

    /// 注册仅接受 QUERY 的路由，并返回可配置的路由对象。
    #[track_caller]
    pub fn query<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(
            &[Method::from_bytes(b"QUERY").expect("有效的 QUERY 方法")],
            uri,
            handler,
        )
    }

    /// 注册框架支持的全部八种方法，不包含 TRACE 或 CONNECT。
    #[track_caller]
    pub fn any<Arguments>(
        &mut self,
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&Self::all_methods(), uri, handler)
    }

    pub(crate) fn all_methods() -> [Method; VERBS.len()] {
        std::array::from_fn(|index| {
            Method::from_bytes(VERBS[index].as_bytes()).expect("有效的 HTTP 方法")
        })
    }

    /// 注册 GET/HEAD 后备路由；仍继承组的前缀、域名和中间件。
    #[track_caller]
    pub fn fallback<Arguments>(
        &mut self,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<&mut Route, RoutingError> {
        let route = self.get("/{fallbackPlaceholder}", handler)?;
        route.r#where("fallbackPlaceholder", ".*")?;
        Ok(route.fallback())
    }

    /// 注册直接渲染视图的 GET/HEAD 路由。
    #[track_caller]
    pub fn view(
        &mut self,
        uri: &str,
        view: &str,
        options: super::ViewRouteOptions,
    ) -> Result<&mut Route, RoutingError> {
        self.get(uri, super::ViewController::new(view, options))
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
        let route = Registrar::r#match(self, methods, uri, handler.into_route_handler())?;
        route.definition = definition;
        if !action.contains("{{closure}}") {
            route.action = action;
        }
        Ok(route)
    }

    #[track_caller]
    pub fn redirect(
        &mut self,
        uri: &str,
        target: &str,
        status: crate::http::StatusCode,
    ) -> Result<&mut Route, RoutingError> {
        if uri.split('{').skip(1).any(|tail| {
            tail.split(['}', ':', '?'])
                .next()
                .is_some_and(|name| matches!(name, "destination" | "status"))
        }) {
            return Err(RoutingError::InvalidUri {
                uri: uri.to_owned(),
            });
        }
        let methods = VERBS
            .iter()
            .map(|verb| Method::from_bytes(verb.as_bytes()).expect("valid HTTP method"))
            .collect::<Vec<_>>();
        self.add_route(
            &methods,
            uri,
            Arc::new(super::redirector::RedirectRoute::new(target, status)),
        )
    }

    #[track_caller]
    pub fn permanent_redirect(
        &mut self,
        uri: &str,
        target: &str,
    ) -> Result<&mut Route, RoutingError> {
        self.redirect(uri, target, crate::http::StatusCode::MOVED_PERMANENTLY)
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
        permissions: crate::filesystem::LocalPermissions,
    ) -> Result<(), RoutingError> {
        self.static_mounts
            .push(StaticMount::public_disk(prefix, directory, permissions)?);
        Ok(())
    }

    pub(crate) fn resolve_static_directories(&mut self, base: &Path) {
        for mount in &mut self.static_mounts {
            mount.resolve_directory(base);
        }
    }

    pub(crate) fn parameter_bindings(&self) -> &HashMap<String, Resolver> {
        &self.bindings
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
        locale: &crate::http::middleware::SetLocale,
    ) -> Result<(), RoutingError> {
        self.locale_middleware = vec![Arc::new(locale.clone())];
        let registry = self.middleware_registry(registry);
        let groups = registry.resolve_groups()?;
        let active_routes: HashSet<_> = self.by_method.values().flatten().copied().collect();
        for index in active_routes {
            let route = &mut self.routes[index];
            let mut stack =
                registry.resolve_stack(&route.middleware, &route.excluded_middleware, &groups)?;
            // 同一默认实例在路由管线选择一次语言，Web 必须先完成 Cookie 解密与队列装配。
            let position = stack.iter().rposition(|item| {
                #[cfg(feature = "sanctum")]
                if item.middleware_type_id() == std::any::TypeId::of::<crate::sanctum::EnsureFrontendRequestsAreStateful>() {
                    return true;
                }
                item.middleware_type_id() == std::any::TypeId::of::<crate::cookie::middleware::AddQueuedCookiesToResponse>()
                || item.middleware_type_id() == std::any::TypeId::of::<crate::cookie::middleware::EncryptCookies>()
            }).map_or(0, |index| index + 1);
            stack.insert(position, Arc::new(locale.clone()));
            route.resolved_middleware = Some(stack);
        }
        Ok(())
    }

    pub(crate) fn storage_route(
        &mut self,
        uri: &str,
        name: &str,
        upload: bool,
        handler: impl Handler + 'static,
    ) -> Result<(), RoutingError> {
        let methods: &[Method] = if upload {
            &[Method::PUT]
        } else {
            &[Method::GET, Method::HEAD]
        };
        let route = self.add_route(methods, uri, Arc::new(handler))?;
        route.r#where("path", ".+")?.name(name);
        route.public_files_first = !upload;
        Ok(())
    }

    async fn static_response(
        &self,
        path: &str,
        method: &Method,
        headers: &crate::http::HeaderMap,
        trailing_slash: bool,
    ) -> Option<Response> {
        if !matches!(*method, Method::GET | Method::HEAD) || trailing_slash {
            return None;
        }
        for mount in &self.static_mounts {
            if let Some(response) = mount.serve(path, method, headers).await {
                return Some(response);
            }
        }
        None
    }

    fn matched(&self, method: &Method, host: &str, path: &str) -> Option<&Route> {
        let routes = self.by_method.get(method)?;
        let matches = |&index: &usize| self.routes[index].matches(host, path);
        let mut fallback = None;
        let mut select = |index| {
            if !matches(&index) {
                return false;
            }
            if self.routes[index].is_fallback {
                fallback.get_or_insert(index);
                return false;
            }
            true
        };
        let index = if let Some(cache) = &self.route_cache {
            cache
                .candidates(method, path)
                .map(|position| routes[position])
                .find(|&index| select(index))
        } else {
            routes.iter().copied().find(|&index| select(index))
        }
        .or(fallback)?;
        Some(&self.routes[index])
    }

    pub async fn dispatch(&self, request: &mut Request) -> Result<Response, HttpError> {
        request.set_route_parameters(Vec::new());
        request.route_terminators.clear();
        request.extensions_mut().remove::<super::RouteMetadata>();
        request.matched_route = false;
        request.route_name = None;
        request
            .extensions_mut()
            .remove::<super::middleware::MatchedRouteDomain>();
        request.session_block = None;
        request.apply_method_override().await?;
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
            if route.public_files_first
                && request.uri().query().is_none_or(|query| {
                    !query.split('&').any(|part| part.starts_with("signature="))
                })
                && let Some(response) = self
                    .static_response(
                        &path,
                        request.method(),
                        request.headers(),
                        request.uri().path().ends_with('/'),
                    )
                    .await
            {
                return Ok(self.localize_response(request, response).await);
            }
            let parameters = route.parameters(host, &path);
            return self.run_route(request, route, parameters).await;
        }

        if self.locale_middleware.is_empty() {
            return self.dispatch_unmatched(request).await;
        }
        Ok(
            Next::new(&self.locale_middleware, &UnmatchedDispatcher(self))
                .run(request)
                .await,
        )
    }

    /// 使用当前请求重新绑定命名路由，并运行目标路由的中间件与处理器。
    pub async fn respond_with_route(
        &self,
        name: &str,
        request: &mut Request,
    ) -> Result<Response, HttpError> {
        let active: HashSet<_> = self.by_method.values().flatten().copied().collect();
        let route = [true, false]
            .into_iter()
            .find_map(|domain| {
                self.routes.iter().enumerate().find_map(|(index, route)| {
                    (route.domain.is_some() == domain
                        && active.contains(&index)
                        && route.get_name() == Some(name))
                    .then_some(route)
                })
            })
            .ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other(format!("命名路由 {name:?} 不存在")).into(),
            })?;
        let path = request.uri().path().trim_end_matches('/');
        let path = percent_decode_str(if path.is_empty() { "/" } else { path })
            .decode_utf8()
            .map_err(|_| HttpError::InvalidRequest)?;
        let host = request
            .uri()
            .host()
            .map(str::to_owned)
            .or_else(|| {
                request
                    .headers()
                    .get(::http::header::HOST)
                    .and_then(|header| header.to_str().ok())
                    .and_then(|host| host.parse::<::http::uri::Authority>().ok())
                    .map(|authority| authority.host().to_owned())
            })
            .unwrap_or_default();
        let path = path.into_owned();
        let parameters = route.parameters(&host, &path);
        self.run_route(request, route, parameters).await
    }

    // 在全局中间件和 RequestReceived 之前识别保护路由；覆盖方法覆写及 405。
    pub(crate) fn protect_sensitive_request(&self, request: &Request) {
        let raw = request.uri().path().trim_end_matches('/');
        let Ok(path) = percent_decode_str(if raw.is_empty() { "/" } else { raw }).decode_utf8()
        else {
            return;
        };
        let authority = request
            .headers()
            .get(::http::header::HOST)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<::http::uri::Authority>().ok());
        let host = request
            .uri()
            .host()
            .or_else(|| authority.as_ref().map(::http::uri::Authority::host))
            .unwrap_or_default();
        for verb in VERBS {
            if let Ok(method) = Method::from_bytes(verb.as_bytes())
                && let Some(route) = self.matched(&method, host, &path)
                && route.resolved_middleware.as_ref().is_some_and(|stack| stack.iter().any(|item| item.middleware_type_id() == std::any::TypeId::of::<crate::http::middleware::ProtectSensitiveData>()))
            {
                crate::http::middleware::ProtectSensitiveData::mark(request, &route.uri);
                return;
            }
        }
    }

    async fn run_route(
        &self,
        request: &mut Request,
        route: &Route,
        parameters: Vec<(String, String)>,
    ) -> Result<Response, HttpError> {
        // 命名分发会替换当前路由；不保留原路由参数、模型、域名和终结器。
        request.route_terminators.clear();
        request
            .extensions_mut()
            .remove::<super::middleware::MatchedRouteDomain>();
        request.extensions_mut().insert(super::RouteMetadata {
            methods: route.methods.clone(),
            uri: route.uri.clone(),
            domain: route.domain.clone(),
            action: route
                .controller_action
                .clone()
                .unwrap_or_else(|| route.action.into()),
            middleware: Vec::new(),
        });
        request.matched_route = true;
        request.route_name = (!route.name.is_empty()).then(|| route.name.clone());
        if let Some(domain) = &route.domain {
            request
                .extensions_mut()
                .insert(super::middleware::MatchedRouteDomain(domain.clone()));
        }
        request.session_block = route.session_block;
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
        if middleware.iter().any(|item| {
            item.middleware_type_id()
                == std::any::TypeId::of::<crate::http::middleware::ProtectSensitiveData>()
        }) {
            crate::http::middleware::ProtectSensitiveData::mark(request, &route.uri);
        }
        if let Some(metadata) = request.extensions_mut().get_mut::<super::RouteMetadata>() {
            metadata.middleware = middleware
                .iter()
                .map(|item| item.middleware_type_name().to_owned())
                .collect();
        }
        request.route_terminators.extend(
            middleware
                .iter()
                .filter(|middleware| middleware.terminable().is_some())
                .cloned(),
        );
        let dispatcher = RouteDispatcher(route.handler.as_ref());
        Ok(Next::new(middleware, &dispatcher).run(request).await)
    }

    async fn localize_response(&self, request: &mut Request, response: Response) -> Response {
        // Handler 通过共享引用调用，响应体则必须按所有权移交；这个容器只属于当前请求。
        let handler = StaticResponseDispatcher(std::sync::Mutex::new(Some(response)));
        Next::new(&self.locale_middleware, &handler)
            .run(request)
            .await
    }

    async fn dispatch_unmatched(&self, request: &mut Request) -> Result<Response, HttpError> {
        let path = percent_decode_str(request.uri().path().trim_end_matches('/'))
            .decode_utf8()
            .map_err(|_| HttpError::InvalidRequest)?;
        let path = if path.is_empty() { "/" } else { &path };
        let authority = request
            .headers()
            .get(::http::header::HOST)
            .and_then(|header| header.to_str().ok())
            .and_then(|host| host.parse::<::http::uri::Authority>().ok());
        let host = request
            .uri()
            .host()
            .or_else(|| authority.as_ref().map(::http::uri::Authority::host))
            .unwrap_or_default();
        let allowed: Vec<_> = VERBS
            .iter()
            .map(|verb| Method::from_bytes(verb.as_bytes()).expect("valid HTTP method"))
            .filter(|method| self.matched(method, host, path).is_some())
            .collect();
        if allowed.is_empty() {
            if let Some(response) = self
                .static_response(
                    path,
                    request.method(),
                    request.headers(),
                    request.uri().path().ends_with('/'),
                )
                .await
            {
                return Ok(response);
            }
            return Err(HttpError::NotFound);
        }
        if request.method() == Method::OPTIONS {
            return Ok(Response::empty(StatusCode::OK).with_allow(&allowed, ","));
        }
        Err(HttpError::MethodNotAllowed { allowed })
    }
}

struct StaticResponseDispatcher(std::sync::Mutex<Option<Response>>);
impl Handler for StaticResponseDispatcher {
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(std::future::ready(Ok(self
            .0
            .lock()
            .expect("响应锁不可用")
            .take()
            .expect("响应只发送一次"))))
    }
}

// 未匹配路由的错误页及公开静态资源仍执行默认语言处理，不启用 Web Cookie 管线。
struct UnmatchedDispatcher<'a>(&'a Router);
impl Handler for UnmatchedDispatcher<'_> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(self.0.dispatch_unmatched(request))
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
        let route = self.prepare_route(uri, handler)?;
        Ok(self.insert_route(methods, route))
    }
}

impl Router {
    #[track_caller]
    pub(super) fn prepare_route(
        &self,
        uri: &str,
        handler: Arc<dyn Handler>,
    ) -> Result<Route, RoutingError> {
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
        Ok(route)
    }

    pub(super) fn insert_route(&mut self, methods: &[Method], mut route: Route) -> &mut Route {
        let head = (methods.contains(&Method::GET) && !methods.contains(&Method::HEAD))
            .then_some(Method::HEAD);
        let all_methods = methods.iter().chain(head.iter());
        // 元数据保留原始方法顺序和重复项；补充的 HEAD 仍在最后。
        let mut metadata = Vec::with_capacity(methods.len() + usize::from(head.is_some()));
        metadata.extend(all_methods.clone().map(|method| method.as_str().to_owned()));
        route.methods = metadata;
        self.route_cache = None;
        self.has_domains |= route.domain.is_some();
        let index = self.routes.len();
        // 常用短方法列表直接借用输入；长列表保留哈希去重，避免平方级扫描。
        let mut seen = (methods.len() > VERBS.len())
            .then(|| HashSet::with_capacity(methods.len() + usize::from(head.is_some())));
        for (position, method) in all_methods.enumerate() {
            let duplicate = match &mut seen {
                Some(seen) => !seen.insert(method),
                None => methods[..position].contains(method),
            };
            if duplicate {
                continue;
            }
            let routes = self.by_method.entry(method.clone()).or_default();
            if let Some(slot) = routes.iter_mut().find(|&&mut entry| {
                let previous = &self.routes[entry];
                previous.registration_uri() == route.registration_uri()
                    && previous.domain == route.domain
            }) {
                *slot = index;
            } else {
                routes.push(index);
            }
        }
        self.routes.push(route);
        &mut self.routes[index]
    }
}
