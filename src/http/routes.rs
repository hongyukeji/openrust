//! 基于 [`axum::Router`] 的路由约定。

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use crate::core::AppState;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::handler::Handler;
use axum::http::{
    HeaderMap, Method, StatusCode, Uri, Version,
    header::{ALLOW, HOST},
};
use axum::response::{IntoResponse, Response};
use axum::routing::{self, MethodRouter, Route};
use tower::ServiceExt;
use tower::{Layer, Service};

use crate::http::domain::DomainPattern;
use crate::http::resource::{Resource, ResourceActions};

type GroupMiddleware = Box<dyn FnOnce(Routes, Option<&MiddlewareRegistry>) -> Routes + Send + Sync>;

pub(crate) type MiddlewareLayerBuilder = Arc<dyn Fn(Routes) -> Routes + Send + Sync>;
pub(crate) struct NamedRouteDefinition {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) domain: Option<String>,
}

struct RootEntry {
    router: Router<AppState>,
    paths: Vec<String>,
    order: u64,
}

struct FallbackEntry {
    router: Router<AppState>,
    scope: String,
    order: u64,
}

enum MergeEntry {
    Root(RootEntry),
    Domain(DomainEntry),
    Fallback(FallbackEntry),
}

#[derive(Clone)]
pub(crate) struct DomainEntry {
    pub(crate) pattern: DomainPattern,
    pub(crate) router: Router<AppState>,
    pub(crate) fallback: Router<AppState>,
    pub(crate) fallback_scopes: Vec<String>,
    paths: Vec<String>,
    order: u64,
}

pub(crate) struct RouteParts {
    pub(crate) router: Router<AppState>,
    fallbacks: Vec<FallbackEntry>,
    pub(crate) domains: Vec<DomainEntry>,
    pub(crate) names: Vec<NamedRouteDefinition>,
    pub(crate) name_errors: Vec<String>,
    pub(crate) middleware_errors: Vec<crate::core::Error>,
    pub(crate) route_errors: Vec<crate::core::Error>,
}

/// 应用启动时登记、路由构建时读取的命名 middleware 定义。
#[derive(Default, Clone)]
pub(crate) struct MiddlewareRegistry {
    aliases: HashMap<String, MiddlewareLayerBuilder>,
    groups: HashMap<String, Vec<String>>,
}

impl MiddlewareRegistry {
    pub(crate) fn alias(
        &mut self,
        name: String,
        builder: MiddlewareLayerBuilder,
    ) -> crate::core::Result<()> {
        self.validate_new_name(&name)?;
        self.aliases.insert(name, builder);
        Ok(())
    }

    pub(crate) fn group(&mut self, name: String, members: Vec<String>) -> crate::core::Result<()> {
        self.validate_new_name(&name)?;
        if members.is_empty() {
            return Err(crate::core::Error::custom(
                "http_middleware_group_empty",
                "HTTP middleware groups must contain at least one middleware name",
            ));
        }
        if members.iter().any(|member| !valid_middleware_name(member)) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_invalid",
                "HTTP middleware names may contain only ASCII letters, digits, dots, underscores and hyphens",
            ));
        }
        self.groups.insert(name, members);
        Ok(())
    }

    fn validate_new_name(&self, name: &str) -> crate::core::Result<()> {
        if !valid_middleware_name(name) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_invalid",
                "HTTP middleware names may contain only ASCII letters, digits, dots, underscores and hyphens",
            ));
        }
        if self.aliases.contains_key(name) || self.groups.contains_key(name) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_duplicate",
                "HTTP middleware alias or group name is already registered",
            ));
        }
        Ok(())
    }

    fn expand(
        &self,
        name: &str,
        stack: &mut HashSet<String>,
    ) -> crate::core::Result<Vec<MiddlewareLayerBuilder>> {
        if let Some(alias) = self.aliases.get(name) {
            return Ok(vec![Arc::clone(alias)]);
        }
        let Some(members) = self.groups.get(name) else {
            return Err(crate::core::Error::custom(
                "http_middleware_name_unknown",
                format!("HTTP middleware alias or group `{name}` is not registered"),
            ));
        };
        if !stack.insert(name.to_owned()) {
            return Err(crate::core::Error::custom(
                "http_middleware_group_cycle",
                "HTTP middleware groups contain a reference cycle",
            ));
        }
        let mut expanded = Vec::new();
        for member in members {
            expanded.extend(self.expand(member, stack)?);
        }
        stack.remove(name);
        Ok(expanded)
    }

    pub(crate) fn validate(&self) -> crate::core::Result<()> {
        for name in self.groups.keys() {
            let _ = self.expand(name, &mut HashSet::new())?;
        }
        Ok(())
    }
}

fn valid_middleware_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// 一组路由共享的 Laravel 式路径前缀与 middleware。
///
/// 组可嵌套；内层组会继承外层前缀与 middleware。Tower middleware 以声明顺序执行请求。
///
/// ```
/// use openrust::http::{RouteGroup, Routes};
///
/// async fn index() -> &'static str { "posts" }
/// let routes = Routes::new().group(RouteGroup::new().prefix("/api"), |api| {
///     api.get("/posts", index)
/// });
/// ```
#[derive(Default)]
pub struct RouteGroup {
    prefix: String,
    name_prefix: String,
    domain: Option<String>,
    middleware: Vec<GroupMiddleware>,
}

impl RouteGroup {
    /// 创建没有前缀或 middleware 的路由组。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置组路径前缀；嵌套组会与父组路径组合。
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        let trimmed = prefix.trim_matches('/');
        self.prefix = if trimmed.is_empty() {
            String::new()
        } else {
            format!("/{trimmed}")
        };
        self
    }

    /// 为组内路由名增加 Laravel 式原样前缀，例如 `admin.`。
    #[must_use]
    pub fn name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.name_prefix = prefix.into();
        self
    }

    /// 将组内路由限定到完整 DNS 域模式；嵌套组显式设置时覆盖父域。
    #[must_use]
    pub fn domain(mut self, pattern: impl Into<String>) -> Self {
        self.domain = Some(pattern.into());
        self
    }

    /// 将 Tower middleware 应用于组内所有路由。
    ///
    /// 多个 middleware 按调用顺序进入请求；响应按相反顺序返回。
    #[must_use]
    pub fn middleware<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.middleware
            .push(Box::new(move |routes, _registry| routes.layer(layer)));
        self
    }

    /// 按应用 Provider 登记的别名，将 middleware 限定到该组。
    #[must_use]
    pub fn middleware_alias(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.middleware.push(Box::new(move |routes, registry| {
            apply_named_middleware(routes, registry, &name)
        }));
        self
    }

    /// 将应用登记的 middleware group 限定到该组。
    #[must_use]
    pub fn middleware_group(self, name: impl Into<String>) -> Self {
        self.middleware_alias(name)
    }

    fn apply(self, mut routes: Routes, registry: Option<&MiddlewareRegistry>) -> Routes {
        for middleware in self.middleware.into_iter().rev() {
            routes = middleware(routes, registry);
        }
        routes
    }
}

/// 收集应用路由；服务器启动时会将其转换为 [`axum::Router`]。
///
/// 每个方法都会消费并返回 `Self`，使路由表可按从上到下的顺序阅读：
///
/// ```
/// use openrust::http::Routes;
///
/// async fn index() -> &'static str { "posts" }
/// async fn show() -> &'static str { "post" }
///
/// fn register(routes: Routes) -> Routes {
///     routes.group(openrust::http::RouteGroup::new().prefix("/api"), |api| {
///         api.get("/posts", index).name("posts.index").get("/posts/{id}", show)
///     })
/// }
/// ```
#[derive(Default)]
pub struct Routes {
    router: Router<AppState>,
    fallback_router: Router<AppState>,
    last_route: Option<(String, String, Option<String>)>,
    route_names: Vec<NamedRouteDefinition>,
    active_domain: Option<DomainPattern>,
    domain_entries: Vec<DomainEntry>,
    root_entries: Vec<RootEntry>,
    fallback_entries: Vec<FallbackEntry>,
    registration_order: Arc<std::sync::atomic::AtomicU64>,
    first_route_order: Option<u64>,
    route_name_errors: Vec<String>,
    middleware_registry: Option<Arc<MiddlewareRegistry>>,
    middleware_errors: Vec<crate::core::Error>,
    route_errors: Vec<crate::core::Error>,
    fallback_registered: bool,
    fallback_prefixes: Vec<String>,
}

impl Routes {
    /// 创建空路由表。
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_middleware_registry(registry: Arc<MiddlewareRegistry>) -> Self {
        Self {
            middleware_registry: Some(registry),
            ..Self::default()
        }
    }

    /// 使用显式 [`MethodRouter`] 添加路由。
    #[must_use]
    pub fn route(mut self, path: &str, method_router: MethodRouter<AppState>) -> Self {
        let order = self
            .registration_order
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.first_route_order.get_or_insert(order);
        if let Some(pattern) = self.active_domain.clone() {
            self.add_domain(DomainEntry {
                pattern,
                router: Router::new().route(path, method_router),
                fallback: Router::new(),
                fallback_scopes: Vec::new(),
                paths: vec![path.to_owned()],
                order,
            });
        } else {
            self.root_entries.push(RootEntry {
                router: Router::new().route(path, method_router.clone()),
                paths: vec![path.to_owned()],
                order,
            });
            self.router = self.router.route(path, method_router);
        }
        self.last_route = Some((
            path.to_owned(),
            String::new(),
            self.active_domain
                .as_ref()
                .map(|domain| domain.as_str().to_owned()),
        ));
        self
    }

    /// 为最近注册的路由指定唯一名称，供 [`crate::http::RouteUrlGenerator`] 生成 URL。
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if let Some((path, prefix, domain)) = self.last_route.take() {
            self.route_names.push(NamedRouteDefinition {
                name: format!("{prefix}{name}"),
                path,
                domain,
            });
        } else {
            self.route_name_errors.push(name);
        }
        self
    }

    /// 注册 `GET` 路由。
    #[must_use]
    pub fn get<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::get(handler))
    }

    /// 注册 `POST` 路由。
    #[must_use]
    pub fn post<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::post(handler))
    }

    /// 注册 `PUT` 路由。
    #[must_use]
    pub fn put<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::put(handler))
    }

    /// 注册 `PATCH` 路由。
    #[must_use]
    pub fn patch<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::patch(handler))
    }

    /// 注册 `DELETE` 路由。
    #[must_use]
    pub fn delete<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::delete(handler))
    }

    /// 注册资源控制器显式提供的操作。
    ///
    /// # Panics
    ///
    /// 若路由与现有路由冲突（路径和方法相同，或同一位置的参数名称不同），Axum 会在构建时 panic。
    /// 经 `BootContext::routes` 注册时，`build_router` 会将此情况转换为启动错误；直接使用
    /// 本类型或 Axum 构建 API 时仍由调用方负责处理 panic。成员路由始终使用参数名 `id`。
    #[must_use]
    pub fn resource<R: Resource>(self, path: &str) -> Self {
        self.resource_with::<R>(path, |actions| actions)
    }

    /// 将 `f` 应用于资源操作后注册资源。
    ///
    /// # Panics
    ///
    /// 若路由与现有路由冲突（路径和方法相同，或同一位置的参数名称不同），Axum 会在构建时 panic。
    /// 经 `BootContext::routes` 注册时，`build_router` 会将此情况转换为启动错误；直接使用
    /// 本类型或 Axum 构建 API 时仍由调用方负责处理 panic。成员路由始终使用参数名 `id`。
    #[must_use]
    pub fn resource_with<R: Resource>(
        self,
        path: &str,
        f: impl FnOnce(ResourceActions) -> ResourceActions,
    ) -> Self {
        f(R::actions(ResourceActions::new())).mount(self, path)
    }

    /// 将 `f` 构建的路由嵌套在 `prefix` 下。空前缀或 `/` 会改为合并路由。
    #[must_use]
    pub fn nest(self, prefix: &str, f: impl FnOnce(Routes) -> Routes) -> Self {
        self.group(RouteGroup::new().prefix(prefix), f)
    }

    /// 以共享属性注册一组嵌套路由。
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "域、路径前缀、fallback 与 middleware 的组合顺序须在同一处核对"
    )]
    pub fn group(mut self, group: RouteGroup, f: impl FnOnce(Routes) -> Routes) -> Self {
        let prefix = group.prefix.clone();
        let name_prefix = group.name_prefix.clone();
        let domain = match group.domain.clone() {
            Some(pattern) => match DomainPattern::parse(&pattern) {
                Ok(pattern) => Some(pattern),
                Err(error) => {
                    self.route_errors.push(error);
                    return self;
                }
            },
            None => self.active_domain.clone(),
        };
        let child = Routes {
            middleware_registry: self.middleware_registry.clone(),
            active_domain: domain.clone(),
            registration_order: Arc::clone(&self.registration_order),
            ..Routes::new()
        };
        let mut inner = group.apply(f(child), self.middleware_registry.as_deref());
        for named in &mut inner.route_names {
            named.name = format!("{name_prefix}{}", named.name);
            named.path = join_route_path(&prefix, &named.path);
        }
        if let Some((path, pending_name_prefix, _)) = &mut inner.last_route {
            *path = join_route_path(&prefix, path);
            *pending_name_prefix = format!("{name_prefix}{pending_name_prefix}");
        }
        let inner_fallback_prefixes: Vec<_> = inner
            .fallback_prefixes
            .iter()
            .map(|nested| fallback_scope_key(&join_route_path(&prefix, nested)))
            .collect();
        let invalid_fallback_scope = inner_fallback_prefixes
            .iter()
            .any(|scope| fallback_scope_unsupported(scope))
            || inner.domain_entries.iter().any(|entry| {
                entry.fallback_scopes.iter().any(|scope| {
                    fallback_scope_unsupported(&fallback_scope_key(&join_route_path(
                        &prefix, scope,
                    )))
                })
            });
        if invalid_fallback_scope {
            self.route_errors.push(crate::core::Error::custom(
                "http_fallback_scope_unsupported",
                "fallback routes cannot use parameterized or wildcard group prefixes",
            ));
            return self;
        }
        let nested_router = mount_prefix(&prefix, inner.router);
        let nested_fallback = mount_prefix(&prefix, inner.fallback_router);
        if domain.is_none() {
            if inner_fallback_prefixes
                .iter()
                .any(|scope| self.fallback_prefixes.contains(scope))
            {
                self.route_errors.push(fallback_duplicate());
            } else {
                self.fallback_registered |=
                    inner_fallback_prefixes.iter().any(|scope| scope == "/");
                self.fallback_prefixes.extend(inner_fallback_prefixes);
                self.router = self.router.merge(nested_router);
                self.fallback_router = self.fallback_router.merge(nested_fallback);
                self.first_route_order = self.first_route_order.or(inner.first_route_order);
                for mut entry in inner.root_entries.drain(..) {
                    entry.router = mount_prefix(&prefix, entry.router);
                    entry.paths = entry
                        .paths
                        .iter()
                        .map(|path| join_route_path(&prefix, path))
                        .collect();
                    self.root_entries.push(entry);
                }
                for mut entry in inner.fallback_entries.drain(..) {
                    entry.scope = fallback_scope_key(&join_route_path(&prefix, &entry.scope));
                    self.fallback_entries.push(entry);
                }
            }
        } else if let Some(pattern) = domain {
            for mut entry in inner.root_entries.drain(..) {
                entry.router = mount_prefix(&prefix, entry.router);
                self.add_domain(DomainEntry {
                    pattern: pattern.clone(),
                    router: entry.router,
                    fallback: Router::new(),
                    fallback_scopes: Vec::new(),
                    paths: entry
                        .paths
                        .iter()
                        .map(|path| join_route_path(&prefix, path))
                        .collect(),
                    order: entry.order,
                });
            }
            for entry in inner.fallback_entries.drain(..) {
                self.add_domain(DomainEntry {
                    pattern: pattern.clone(),
                    router: Router::new(),
                    fallback: entry.router,
                    fallback_scopes: vec![fallback_scope_key(&join_route_path(
                        &prefix,
                        &entry.scope,
                    ))],
                    paths: Vec::new(),
                    order: entry.order,
                });
            }
            let _ = (nested_router, nested_fallback);
        }
        for mut entry in inner.domain_entries.drain(..) {
            entry.router = mount_prefix(&prefix, entry.router);
            entry.paths = entry
                .paths
                .iter()
                .map(|path| join_route_path(&prefix, path))
                .collect();
            entry.fallback_scopes = entry
                .fallback_scopes
                .iter()
                .map(|scope| fallback_scope_key(&join_route_path(&prefix, scope)))
                .collect();
            self.add_domain(entry);
        }
        self.route_names.extend(inner.route_names);
        self.route_name_errors.extend(inner.route_name_errors);
        self.middleware_errors.extend(inner.middleware_errors);
        self.route_errors.extend(inner.route_errors);
        self.last_route = inner.last_route;
        self
    }

    /// 在同一层级合并另一张路由表。
    #[must_use]
    pub fn merge(mut self, mut other: Routes) -> Self {
        if other
            .fallback_prefixes
            .iter()
            .any(|prefix| self.fallback_prefixes.contains(prefix))
        {
            self.route_errors.push(crate::core::Error::custom(
                "http_fallback_duplicate",
                "only one fallback route can be registered for the same route scope",
            ));
        } else if self.active_domain.is_none() {
            self.fallback_registered |= other.fallback_registered;
            self.fallback_prefixes
                .extend(other.fallback_prefixes.clone());
            self.router = self.router.merge(other.router.clone());
            self.fallback_router = self.fallback_router.merge(other.fallback_router.clone());
        }
        let mut entries: Vec<_> = other
            .root_entries
            .drain(..)
            .map(|entry| (entry.order, MergeEntry::Root(entry)))
            .chain(
                other
                    .domain_entries
                    .drain(..)
                    .map(|entry| (entry.order, MergeEntry::Domain(entry))),
            )
            .chain(
                other
                    .fallback_entries
                    .drain(..)
                    .map(|entry| (entry.order, MergeEntry::Fallback(entry))),
            )
            .collect();
        entries.sort_by_key(|(order, _)| *order);
        for (_, entry) in entries {
            let order = self
                .registration_order
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match entry {
                MergeEntry::Root(mut entry) => {
                    entry.order = order;
                    if let Some(pattern) = self.active_domain.clone() {
                        self.add_domain(DomainEntry {
                            pattern,
                            router: entry.router,
                            fallback: Router::new(),
                            fallback_scopes: Vec::new(),
                            paths: entry.paths,
                            order,
                        });
                    } else {
                        self.root_entries.push(entry);
                    }
                }
                MergeEntry::Domain(mut entry) => {
                    entry.order = order;
                    self.add_domain(entry);
                }
                MergeEntry::Fallback(mut entry) => {
                    entry.order = order;
                    if let Some(pattern) = self.active_domain.clone() {
                        self.add_domain(DomainEntry {
                            pattern,
                            router: Router::new(),
                            fallback: entry.router,
                            fallback_scopes: vec![entry.scope],
                            paths: Vec::new(),
                            order,
                        });
                    } else {
                        self.fallback_entries.push(entry);
                    }
                }
            }
        }
        if let Some(pattern) = &self.active_domain {
            for named in &mut other.route_names {
                if named.domain.is_none() {
                    named.domain = Some(pattern.as_str().to_owned());
                }
            }
        }
        self.route_names.extend(other.route_names);
        self.route_name_errors.extend(other.route_name_errors);
        self.middleware_errors.extend(other.middleware_errors);
        self.route_errors.extend(other.route_errors);
        self.last_route = other.last_route.map(|(path, prefix, domain)| {
            (
                path,
                prefix,
                domain.or_else(|| {
                    self.active_domain
                        .as_ref()
                        .map(|pattern| pattern.as_str().to_owned())
                }),
            )
        });
        self
    }

    fn add_domain(&mut self, entry: DomainEntry) {
        if entry
            .fallback_scopes
            .iter()
            .any(|scope| fallback_scope_unsupported(scope))
        {
            self.route_errors.push(crate::core::Error::custom(
                "http_fallback_scope_unsupported",
                "fallback routes cannot use parameterized or wildcard group prefixes",
            ));
            return;
        }
        for label in entry.pattern.as_str().split('.') {
            if let Some(name) = label
                .strip_prefix('{')
                .and_then(|label| label.strip_suffix('}'))
            {
                let parameter = format!("{{{name}}}");
                if entry
                    .paths
                    .iter()
                    .any(|path| path.split('/').any(|part| part == parameter))
                {
                    self.route_errors.push(crate::core::Error::custom(
                        "http_route_parameter_duplicate",
                        "domain and path parameter names must differ",
                    ));
                    return;
                }
            }
        }
        if self.active_domain.as_ref().map(DomainPattern::as_str) == Some(entry.pattern.as_str())
            && entry
                .fallback_scopes
                .iter()
                .any(|scope| self.fallback_prefixes.contains(scope))
        {
            self.route_errors.push(fallback_duplicate());
            return;
        }
        for existing in &self.domain_entries {
            if existing.pattern.as_str() != entry.pattern.as_str() {
                continue;
            }
            if entry
                .fallback_scopes
                .iter()
                .any(|scope| existing.fallback_scopes.contains(scope))
            {
                self.route_errors.push(fallback_duplicate());
                return;
            }
            // 同一规范域仍须用 Axum 的重叠规则验证，但分派 bucket 不能合并，
            // 否则后登记的路由会越过中间其他可匹配域的路由。
            let _ = existing.router.clone().merge(entry.router.clone());
        }
        self.domain_entries.push(entry);
    }

    /// 注册未匹配路由时执行的 fallback handler。
    ///
    /// GET/HEAD 请求先尝试读取 `public` 目录中的静态资源，再调用 handler；其他方法直接调用 handler。
    /// 若用于 `RouteGroup` 内，fallback 只覆盖该组前缀，并遵循组的 middleware。已匹配路径上的
    /// 方法不匹配仍由框架返回 405。每个同层 router 只能注册一个 fallback；带路由参数或通配符的组前缀
    /// 暂不支持 fallback，构建时返回 `http_fallback_scope_unsupported`。
    #[must_use]
    pub fn fallback<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState> + Clone,
        T: 'static,
    {
        if self.fallback_registered {
            self.route_errors.push(crate::core::Error::custom(
                "http_fallback_duplicate",
                "only one fallback route can be registered for the same route scope",
            ));
            return self;
        }
        self.fallback_registered = true;
        self.fallback_prefixes.push("/".to_owned());
        let order = self
            .registration_order
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let fallback =
            Router::new().fallback(move |State(state): State<AppState>, mut request: Request| {
                let handler = handler.clone();
                async move {
                    if let Some(original_uri) = request
                        .extensions()
                        .get::<axum::extract::OriginalUri>()
                        .map(|original_uri| original_uri.0.clone())
                    {
                        *request.uri_mut() = original_uri;
                    }
                    if matches!(*request.method(), Method::GET | Method::HEAD) {
                        let static_request = static_request(&request);
                        if let Some(response) =
                            crate::http::site::try_static_file(state.clone(), static_request).await
                        {
                            return response;
                        }
                    }
                    handler.call(request, state).await.into_response()
                }
            });
        self.fallback_router = self.fallback_router.merge(fallback.clone());
        self.fallback_entries.push(FallbackEntry {
            router: fallback,
            scope: "/".to_owned(),
            order,
        });
        self
    }

    /// 将已登记的 middleware alias 应用于截至当前已注册的路由。
    #[must_use]
    pub fn middleware_alias(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if let Some(registry) = self.middleware_registry.clone() {
            self = apply_named_middleware(self, Some(&registry), &name);
        } else {
            self = apply_named_middleware(self, None, &name);
        }
        self
    }

    /// 将已登记的 middleware group 应用于截至当前已注册的路由。
    #[must_use]
    pub fn middleware_group(self, name: impl Into<String>) -> Self {
        self.middleware_alias(name)
    }

    /// 为截至当前已注册的所有路由应用 `tower` layer。
    #[must_use]
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.router = self.router.layer(layer.clone());
        self.fallback_router = self.fallback_router.layer(layer.clone());
        for entry in &mut self.root_entries {
            entry.router = entry.router.clone().layer(layer.clone());
        }
        for entry in &mut self.fallback_entries {
            entry.router = entry.router.clone().layer(layer.clone());
        }
        for entry in &mut self.domain_entries {
            entry.router = entry.router.clone().layer(layer.clone());
            entry.fallback = entry.fallback.clone().layer(layer.clone());
        }
        self
    }

    /// 将收集到的路由交给 axum。
    ///
    /// # Errors
    ///
    /// 若命名 middleware 缺少应用注册表或路由名称不完整，则返回对应配置错误。
    pub fn into_router(self) -> crate::core::Result<Router<AppState>> {
        let mut parts = self.into_parts();
        if !parts.name_errors.is_empty() {
            return Err(crate::core::Error::custom(
                "http_route_name_without_route",
                "a route name was assigned before a route was registered",
            ));
        }
        if !parts.route_errors.is_empty() {
            return Err(parts.route_errors.remove(0));
        }
        if !parts.middleware_errors.is_empty() {
            return Err(parts.middleware_errors.remove(0));
        }
        Ok(assemble(parts))
    }

    pub(crate) fn into_parts(self) -> RouteParts {
        let root = self.active_domain.is_none();
        let mut domains = self.domain_entries;
        if let Some(pattern) = self.active_domain {
            domains.push(DomainEntry {
                pattern,
                router: self.router.clone(),
                fallback: self.fallback_router.clone(),
                fallback_scopes: self.fallback_prefixes.clone(),
                paths: Vec::new(),
                order: self.first_route_order.unwrap_or_default(),
            });
        }
        domains.sort_by_key(|entry| entry.order);
        RouteParts {
            router: if root { self.router } else { Router::new() },
            fallbacks: if root {
                self.fallback_entries
            } else {
                Vec::new()
            },
            domains,
            names: self.route_names,
            name_errors: self.route_name_errors,
            middleware_errors: self.middleware_errors,
            route_errors: self.route_errors,
        }
    }
}

fn static_request(request: &Request) -> Request {
    let mut static_request = Request::new(Body::empty());
    *static_request.method_mut() = request.method().clone();
    *static_request.uri_mut() = request.uri().clone();
    *static_request.version_mut() = request.version();
    *static_request.headers_mut() = request.headers().clone();
    static_request
}

fn apply_named_middleware(
    mut routes: Routes,
    registry: Option<&MiddlewareRegistry>,
    name: &str,
) -> Routes {
    let Some(registry) = registry else {
        routes.middleware_errors.push(crate::core::Error::custom(
            "http_middleware_registry_unavailable",
            "named HTTP middleware requires an application middleware registry",
        ));
        return routes;
    };
    match registry.expand(name, &mut HashSet::new()) {
        Ok(builders) => {
            for builder in builders.into_iter().rev() {
                routes = builder(routes);
            }
        }
        Err(error) => routes.middleware_errors.push(error),
    }
    routes
}

fn join_route_path(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_matches('/');
    if path == "//" {
        return if prefix.is_empty() {
            "//".to_owned()
        } else {
            format!("/{prefix}//")
        };
    }
    let trailing_slash = path.ends_with('/');
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = if trailing_slash {
        path.strip_suffix('/').unwrap_or(path)
    } else {
        path
    };
    let mut joined = match (prefix.is_empty(), path.is_empty()) {
        (true, true) => "/".to_owned(),
        (true, false) => format!("/{path}"),
        (false, true) => format!("/{prefix}"),
        (false, false) => format!("/{prefix}/{path}"),
    };
    if trailing_slash && joined != "/" {
        joined.push('/');
    }
    joined
}

fn fallback_scope_key(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "{}"
            } else if segment.starts_with('*') {
                "*"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn fallback_scope_unsupported(scope: &str) -> bool {
    scope
        .split('/')
        .any(|segment| segment == "{}" || segment == "*")
}

fn mount_prefix(prefix: &str, router: Router<AppState>) -> Router<AppState> {
    if prefix.is_empty() || prefix == "/" {
        router
    } else {
        Router::new().nest(prefix, router)
    }
}

fn fallback_duplicate() -> crate::core::Error {
    crate::core::Error::custom(
        "http_fallback_duplicate",
        "only one fallback route can be registered for the same route scope",
    )
}

#[derive(Clone, Copy)]
enum InternalMiss {
    Path,
    Method,
}

#[derive(Clone)]
struct MissSlot(Arc<Mutex<Option<Request>>>);

#[derive(Clone)]
struct UnmatchedRequest(Arc<Mutex<Option<Request>>>);

/// 仅供站点层在全部业务路由和用户 fallback 确认未匹配后取回原请求。
pub(crate) fn take_unmatched(response: &mut Response) -> Option<Request> {
    response
        .extensions_mut()
        .remove::<UnmatchedRequest>()?
        .0
        .lock()
        .ok()?
        .take()
}

fn missed(mut request: Request, kind: InternalMiss) -> Response {
    if let Some(slot) = request.extensions_mut().remove::<MissSlot>()
        && let Ok(mut pending) = slot.0.lock()
    {
        *pending = Some(request);
    }
    let mut response = Response::new(Body::empty());
    *response.status_mut() = match kind {
        InternalMiss::Path => StatusCode::NOT_FOUND,
        InternalMiss::Method => StatusCode::METHOD_NOT_ALLOWED,
    };
    response.extensions_mut().insert(kind);
    response
}

fn candidate(router: Router<AppState>) -> Router<AppState> {
    router
        .fallback(|request: Request| async move { missed(request, InternalMiss::Path) })
        .method_not_allowed_fallback(|request: Request| async move {
            missed(request, InternalMiss::Method)
        })
}

#[derive(Clone)]
struct RequestSnapshot {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    version: Version,
    extensions: axum::http::Extensions,
}

impl RequestSnapshot {
    fn capture(request: &Request) -> Self {
        Self {
            method: request.method().clone(),
            uri: request.uri().clone(),
            headers: request.headers().clone(),
            version: request.version(),
            extensions: request.extensions().clone(),
        }
    }

    fn restore(&self, request: &mut Request) {
        *request.method_mut() = self.method.clone();
        *request.uri_mut() = self.uri.clone();
        *request.headers_mut() = self.headers.clone();
        *request.version_mut() = self.version;
        *request.extensions_mut() = self.extensions.clone();
    }
}

fn authority(request: &Request) -> crate::core::Result<String> {
    use std::str::FromStr;
    let mut hosts = request.headers().get_all(HOST).iter();
    let header = hosts.next();
    if hosts.next().is_some() {
        return Err(authority_error());
    }
    let header = header
        .map(|value| value.to_str().map_err(|_| authority_error()))
        .transpose()?;
    let uri_authority = request
        .uri()
        .authority()
        .map(axum::http::uri::Authority::as_str);
    let selected = header.or(uri_authority).ok_or_else(authority_error)?;
    let parse = |value: &str| -> crate::core::Result<(String, Option<u16>)> {
        if value.is_empty()
            || value.contains(',')
            || value.contains('@')
            || value.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            return Err(authority_error());
        }
        let parsed = axum::http::uri::Authority::from_str(value).map_err(|_| authority_error())?;
        let host = parsed.host();
        if host.is_empty() || host.ends_with('.') || !host.is_ascii() {
            return Err(authority_error());
        }
        let port = parsed.port_u16();
        if if value.starts_with('[') {
            let Some((address, suffix)) = value.split_once(']') else {
                return Err(authority_error());
            };
            address[1..].parse::<std::net::Ipv6Addr>().is_err()
                || (!suffix.is_empty() && (!suffix.starts_with(':') || port.is_none()))
        } else {
            (value.contains(':') && port.is_none())
                || (host.parse::<std::net::IpAddr>().is_err()
                    && DomainPattern::parse(host).is_err())
        } {
            return Err(authority_error());
        }
        if port == Some(0) || value.ends_with(':') {
            return Err(authority_error());
        }
        let normalized = if let Some(address) = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
        {
            format!(
                "[{}]",
                address
                    .parse::<std::net::Ipv6Addr>()
                    .map_err(|_| authority_error())?
            )
        } else {
            host.to_ascii_lowercase()
        };
        Ok((normalized, port))
    };
    let (host, port) = parse(selected)?;
    if let (Some(header), Some(uri_authority)) = (header, uri_authority)
        && parse(header)? != parse(uri_authority)?
    {
        return Err(authority_error());
    }
    let _ = port;
    Ok(host)
}

fn authority_error() -> crate::core::Error {
    crate::core::Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "http_domain_authority_invalid",
        "request authority must contain one valid Host matching the URI authority",
    )
}

/// 在全部候选路由均确认内部 miss 后执行用户 fallback；handler 返回的状态始终终止分派。
#[expect(
    clippy::too_many_lines,
    reason = "路由候选、方法与 fallback 的分派顺序需在同一处可核对"
)]
pub(crate) fn assemble(parts: RouteParts) -> Router<AppState> {
    let domains: Vec<_> = parts
        .domains
        .into_iter()
        .map(|entry| {
            let has_root = entry.fallback_scopes.iter().any(|scope| scope == "/");
            let specificity = entry
                .fallback_scopes
                .iter()
                .map(String::len)
                .max()
                .unwrap_or_default();
            let fallback = if entry.fallback_scopes.len() == 1 {
                mount_prefix(&entry.fallback_scopes[0], entry.fallback)
            } else {
                entry.fallback
            };
            (
                entry.pattern,
                candidate(entry.router),
                if entry.fallback_scopes.is_empty() {
                    None
                } else if has_root {
                    Some(fallback)
                } else {
                    Some(candidate(fallback))
                },
                has_root,
                specificity,
            )
        })
        .collect();
    let root = candidate(parts.router);
    let mut fallback_entries = parts.fallbacks;
    fallback_entries.sort_by_key(|entry| (std::cmp::Reverse(entry.scope.len()), entry.order));
    let root_fallbacks: Vec<_> = fallback_entries
        .into_iter()
        .map(|entry| {
            let direct = entry.scope == "/";
            let router = mount_prefix(&entry.scope, entry.router);
            (if direct { router } else { candidate(router) }, direct)
        })
        .collect();
    Router::new().fallback(move |State(state): State<AppState>, request: Request| {
        let domains = domains.clone();
        let root = root.clone();
        let root_fallbacks = root_fallbacks.clone();
        async move {
            let host = if domains.is_empty() {
                None
            } else {
                match authority(&request) {
                    Ok(host) => Some(host),
                    Err(error) => return error.into_response(),
                }
            };
            let snapshot = RequestSnapshot::capture(&request);
            let mut pending = request;
            let mut allowed = std::collections::BTreeSet::new();
            let mut method_miss = false;
            let mut matched_domains = Vec::new();
            for (pattern, router, fallback, root_fallback, specificity) in &domains {
                let Some(values) = host.as_ref().and_then(|host| pattern.captures(host)) else {
                    continue;
                };
                if let Some(fallback) = fallback.clone() {
                    matched_domains.push((
                        pattern.as_str().to_owned(),
                        *specificity,
                        fallback,
                        values.clone(),
                        *root_fallback,
                    ));
                }
                snapshot.restore(&mut pending);
                crate::http::domain::set_domain_params(pending.extensions_mut(), values);
                match run_candidate(router, state.clone(), pending).await {
                    Ok((request, allow)) => {
                        pending = request;
                        method_miss |= allow.is_some();
                        collect_allow(&mut allowed, allow);
                    }
                    Err(response) => return response,
                }
            }
            snapshot.restore(&mut pending);
            match run_candidate(&root, state.clone(), pending).await {
                Ok((request, allow)) => {
                    pending = request;
                    method_miss |= allow.is_some();
                    collect_allow(&mut allowed, allow);
                }
                Err(response) => return response,
            }
            if method_miss {
                let mut response = crate::core::Error::custom_with_status(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "method not allowed",
                )
                .into_response();
                if let Ok(value) = axum::http::HeaderValue::from_str(
                    &allowed.into_iter().collect::<Vec<_>>().join(", "),
                ) {
                    response.headers_mut().insert(ALLOW, value);
                }
                return response;
            }
            let mut pattern_order = HashMap::new();
            for (index, (pattern, ..)) in matched_domains.iter().enumerate() {
                pattern_order.entry(pattern.clone()).or_insert(index);
            }
            // 域模式之间按首次 fallback 声明顺序；同一域模式内只比较互相覆盖的
            // 静态 scope，较深前缀先处理，避免根 fallback 吞掉其子组 fallback。
            matched_domains.sort_by_key(|(pattern, specificity, ..)| {
                (pattern_order[pattern], std::cmp::Reverse(*specificity))
            });
            for (_, _, fallback, values, direct) in matched_domains {
                snapshot.restore(&mut pending);
                crate::http::domain::set_domain_params(pending.extensions_mut(), values);
                if direct {
                    return fallback
                        .with_state(state.clone())
                        .oneshot(pending)
                        .await
                        .unwrap_or_else(|never| match never {});
                }
                match run_candidate(&fallback, state.clone(), pending).await {
                    Ok((request, _)) => pending = request,
                    Err(response) => return response,
                }
            }
            snapshot.restore(&mut pending);
            for (fallback, direct) in root_fallbacks {
                snapshot.restore(&mut pending);
                if direct {
                    return fallback
                        .with_state(state.clone())
                        .oneshot(pending)
                        .await
                        .unwrap_or_else(|never| match never {});
                }
                match run_candidate(&fallback, state.clone(), pending).await {
                    Ok((request, _)) => pending = request,
                    Err(response) => return response,
                }
            }
            let slot = UnmatchedRequest(Arc::new(Mutex::new(Some(pending))));
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NOT_FOUND;
            response.extensions_mut().insert(slot);
            response
        }
    })
}

fn collect_allow(allowed: &mut std::collections::BTreeSet<String>, allow: Option<String>) {
    if let Some(allow) = allow {
        for method in allow
            .split(',')
            .map(str::trim)
            .filter(|method| !method.is_empty())
        {
            allowed.insert(method.to_owned());
        }
    }
}

#[expect(
    clippy::result_large_err,
    reason = "handler 响应直接作为终止结果返回，避免额外分配"
)]
async fn run_candidate(
    router: &Router<AppState>,
    state: AppState,
    mut request: Request,
) -> std::result::Result<(Request, Option<String>), Response> {
    let slot = MissSlot(Arc::new(Mutex::new(None)));
    request.extensions_mut().insert(slot.clone());
    let response = match router.clone().with_state(state).oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let Some(miss) = response.extensions().get::<InternalMiss>().copied() else {
        return Err(response);
    };
    let request = slot
        .0
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .ok_or_else(|| {
            crate::core::Error::custom(
                "http_route_dispatch_failed",
                "HTTP route candidate did not return the request",
            )
            .into_response()
        })?;
    let allow = if matches!(miss, InternalMiss::Method) {
        Some(
            response
                .headers()
                .get(ALLOW)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        )
    } else {
        None
    };
    Ok((request, allow))
}
