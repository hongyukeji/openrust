//! 基于 [`axum::Router`] 的路由约定。

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use crate::core::AppState;
use axum::Router;
use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, Version, header::ALLOW};
use axum::response::{IntoResponse, Response};
use axum::routing::Route;
use tower::ServiceExt;
use tower::{Layer, Service};

use crate::http::authority::authority;
use crate::http::domain::DomainPattern;
use crate::http::resource::{Resource, ResourceActions};
use crate::http::routing::{self, ManagedHandler, MethodEntry, RouteMethods};

type ManagedEntries = Arc<Mutex<Vec<MethodEntry>>>;

// 声明只保留一份处理器、middleware 与原始 Route 元数据；集合投影仅引用其位置。
struct RouteDeclaration {
    metadata: Arc<Mutex<MatchedRouteMetadata>>,
    entries: ManagedEntries,
}

#[derive(Clone)]
struct DispatchCandidate {
    method: Method,
    domain: Option<DomainPattern>,
    router: Router<AppState>,
    probe: Router<AppState>,
    fallback_scope: Option<String>,
}

const OFFICIAL_METHODS: [&str; 8] = [
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY",
];

#[derive(Clone, Default)]
pub(crate) struct MatchedRouteMetadata {
    name: Option<String>,
    action: Option<String>,
    path: String,
    domain: Option<String>,
    methods: Vec<String>,
    runtime_prefix: String,
    fallback: bool,
}

#[expect(
    clippy::too_many_lines,
    reason = "方法选择、参数绑定和发布必须保持同一可审查调用链"
)]
pub(crate) async fn dispatch_managed(
    request: Request,
    state: AppState,
    entries: &[MethodEntry],
) -> Response {
    let Some(entry) = entries
        .iter()
        .rev()
        .find(|entry| entry.methods.contains(request.method()))
    else {
        let official = [
            "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY",
        ];
        let allow = official
            .into_iter()
            .filter(|method| {
                entries.iter().any(|entry| {
                    entry
                        .methods
                        .iter()
                        .any(|candidate| candidate.as_str() == *method)
                })
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut response = missed(
            request,
            if allow.is_empty() {
                InternalMiss::Path
            } else {
                InternalMiss::Method
            },
        );
        if let Ok(value) = axum::http::HeaderValue::from_str(&allow) {
            response.headers_mut().insert(ALLOW, value);
        }
        return response;
    };
    let Some(metadata) = &entry.metadata else {
        return crate::core::Error::custom("http_route_metadata_missing", "请求路由声明不可用")
            .into_response();
    };
    let metadata = match snapshot_metadata(metadata) {
        Ok(metadata) => metadata,
        Err(error) => return error.into_response(),
    };
    let route = {
        let domain_values = crate::http::domain::route_params(request.extensions());
        let mut parameters = Vec::new();
        if let Some(domain) = &metadata.domain {
            for label in domain.split('.') {
                let Some(name) = label
                    .strip_prefix('{')
                    .and_then(|label| label.strip_suffix('}'))
                else {
                    continue;
                };
                if let Some((_, value)) = domain_values.iter().find(|(key, _)| key == name) {
                    parameters.push((name.to_owned(), serde_json::Value::String(value.clone())));
                }
            }
        }
        if metadata.fallback {
            let original = crate::http::site::routing_uri(&request);
            let prefix = metadata
                .path
                .strip_suffix("/{fallbackPlaceholder}")
                .unwrap_or_default();
            let captured = original
                .path()
                .strip_prefix(prefix)
                .unwrap_or(original.path())
                .trim_start_matches('/');
            if !captured.is_empty() {
                match decode_route_capture(captured) {
                    Some(value) => parameters.push((
                        "fallbackPlaceholder".to_owned(),
                        serde_json::Value::String(value),
                    )),
                    None => {
                        return crate::core::Error::custom_with_status(
                            StatusCode::BAD_REQUEST,
                            "http_route_parameter_invalid",
                            "路由参数不是有效的UTF-8编码",
                        )
                        .into_response();
                    }
                }
            }
        }
        let (mut parts, body) = request.into_parts();
        match axum::extract::RawPathParams::from_request_parts(&mut parts, &state).await {
            Ok(raw) => {
                for (key, value) in &raw {
                    let value = serde_json::Value::String(value.to_owned());
                    if let Some((_, old)) =
                        parameters.iter_mut().find(|(existing, _)| existing == key)
                    {
                        *old = value;
                    } else {
                        parameters.push((key.to_owned(), value));
                    }
                }
            }
            Err(axum::extract::rejection::RawPathParamsRejection::MissingPathParams(_)) => {}
            Err(_) => {
                return crate::core::Error::custom_with_status(
                    StatusCode::BAD_REQUEST,
                    "http_route_parameter_invalid",
                    "路由参数不是有效的UTF-8编码",
                )
                .into_response();
            }
        }
        let mut route = crate::http::Route::new(
            metadata.name.clone(),
            if metadata.path == "/" {
                "/".to_owned()
            } else {
                metadata.path.trim_matches('/').to_owned()
            },
            metadata.domain.clone(),
            metadata.methods.clone(),
            parameters,
        );
        if let Some(action) = &metadata.action {
            route = route.with_action(action.clone());
        }
        (route, Request::from_parts(parts, body))
    };
    let (route, request) = route;
    if let Some(slot) = request.extensions().get::<crate::http::route::RouteSlot>()
        && let Err(error) = slot.publish(route)
    {
        return error.into_response();
    }
    let mut request = request;
    if !metadata.runtime_prefix.is_empty() && !metadata.fallback {
        let current = request.uri().clone();
        let path = route_runtime_path(current.path(), &metadata.runtime_prefix).to_owned();
        let query = current
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        let mut parts = current.into_parts();
        if let Ok(value) = format!("{path}{query}").parse() {
            parts.path_and_query = Some(value);
            if let Ok(uri) = Uri::from_parts(parts) {
                *request.uri_mut() = uri;
            }
        }
    }
    (entry.handler)(request, state).await
}

// 已由 Axum 验证完整路径；按前缀段数裁剪可同时保留静态与参数化组的原 nest URI 语义。
fn route_runtime_path<'a>(path: &'a str, prefix: &str) -> &'a str {
    let mut remaining = path;
    for _ in prefix.split('/').filter(|segment| !segment.is_empty()) {
        let segment = remaining.strip_prefix('/').unwrap_or(remaining);
        remaining = segment
            .find('/')
            .map_or("", |position| &segment[position..]);
    }
    if remaining.is_empty() { "/" } else { remaining }
}

fn snapshot_metadata(
    metadata: &Arc<Mutex<MatchedRouteMetadata>>,
) -> crate::core::Result<MatchedRouteMetadata> {
    let metadata = metadata.lock().map_err(|_| {
        crate::core::Error::custom("http_route_context_poisoned", "请求路由上下文不可用")
    })?;
    Ok(metadata.clone())
}

fn decode_route_capture(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut input = input.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            bytes.push(u8::try_from(high * 16 + low).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

type GroupMiddleware = Box<dyn FnOnce(Routes, Option<&MiddlewareRegistry>) -> Routes + Send + Sync>;

pub(crate) type MiddlewareLayerBuilder = Arc<dyn Fn(Routes) -> Routes + Send + Sync>;
pub(crate) struct ActionRouteDefinition {
    pub(crate) action: String,
    pub(crate) path: String,
    pub(crate) domain: Option<String>,
}

pub(crate) struct NamedRouteDefinition {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) domain: Option<String>,
}

pub(crate) struct RouteParts {
    candidates: Vec<DispatchCandidate>,
    pub(crate) names: Vec<NamedRouteDefinition>,
    pub(crate) actions: Vec<ActionRouteDefinition>,
    pub(crate) get_paths: Vec<String>,
    pub(crate) name_errors: Vec<String>,
    pub(crate) middleware_errors: Vec<crate::core::Error>,
    pub(crate) route_errors: Vec<crate::core::Error>,
}

pub(crate) type MiddlewareParameterFactory =
    Arc<dyn Fn(Vec<String>) -> crate::core::Result<MiddlewareLayerBuilder> + Send + Sync>;

/// 应用启动时登记、路由构建时读取的命名 middleware 定义。
#[derive(Clone)]
pub(crate) struct MiddlewareRegistry {
    aliases: HashMap<String, MiddlewareLayerBuilder>,
    parameter_aliases: HashMap<String, MiddlewareParameterFactory>,
    groups: HashMap<String, Vec<String>>,
    priority: Vec<String>,
    signed_except: Vec<String>,
}

impl Default for MiddlewareRegistry {
    fn default() -> Self {
        Self {
            aliases: HashMap::new(),
            parameter_aliases: HashMap::new(),
            groups: HashMap::new(),
            priority: ["precognitive", "auth", "throttle", "auth.session", "can"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            signed_except: Vec::new(),
        }
    }
}

impl MiddlewareRegistry {
    pub(crate) fn install_web_defaults(
        &mut self,
        cookie: Option<super::cookie::CookieLayer>,
        #[cfg(feature = "session")] session: Option<crate::session::SessionLayer>,
        #[cfg(feature = "session")] forgery: Option<super::forgery::ForgeryRuntime>,
    ) -> crate::core::Result<()> {
        if !self.contains_name("cookies") {
            self.alias(
                "cookies".to_owned(),
                Arc::new(move |mut routes| {
                    if let Some(layer) = &cookie {
                        routes.layer(axum::middleware::from_fn_with_state(
                            layer.clone(),
                            super::cookie::middleware::handle,
                        ))
                    } else {
                        routes.middleware_errors.push(crate::Error::custom(
                            "web_cookie_provider_missing",
                            "Cookie 路由层要求 CookieServiceProvider",
                        ));
                        routes
                    }
                }),
            )?;
        }
        if !self.contains_name("session") {
            #[cfg(feature = "session")]
            let builder: MiddlewareLayerBuilder = Arc::new(move |mut routes| {
                if let Some(layer) = &session {
                    routes.layer(axum::middleware::from_fn_with_state(
                        Some(layer.clone()),
                        crate::session::handle_session,
                    ))
                } else {
                    routes.middleware_errors.push(crate::Error::custom(
                        "web_session_provider_missing",
                        "Session 路由层要求已启用的 SessionServiceProvider",
                    ));
                    routes
                }
            });
            #[cfg(not(feature = "session"))]
            let builder: MiddlewareLayerBuilder = Arc::new(|mut routes| {
                routes.middleware_errors.push(crate::Error::custom(
                    "web_session_feature_missing",
                    "Web 路由组要求 session feature",
                ));
                routes
            });
            self.alias("session".to_owned(), builder)?;
        }
        if !self.contains_name("csrf") {
            #[cfg(feature = "session")]
            let builder: MiddlewareLayerBuilder = Arc::new(move |mut routes| {
                if let Some(runtime) = &forgery {
                    if runtime
                        .cookie
                        .protects_web_cookies(&runtime.session_cookie_name)
                    {
                        return routes.layer(axum::middleware::from_fn_with_state(
                            runtime.clone(),
                            super::forgery::handle,
                        ));
                    }
                    routes.middleware_errors.push(crate::Error::custom(
                        "web_cookie_encryption_missing",
                        "默认 Web 组要求受保护 Cookie 与 Encryptor",
                    ));
                } else {
                    routes.middleware_errors.push(crate::Error::custom(
                        "web_csrf_runtime_missing",
                        "Web CSRF 要求 Cookie 和有效 Session",
                    ));
                }
                routes
            });
            #[cfg(not(feature = "session"))]
            let builder: MiddlewareLayerBuilder = Arc::new(|mut routes| {
                routes.middleware_errors.push(crate::Error::custom(
                    "web_session_feature_missing",
                    "Web CSRF 要求 session feature",
                ));
                routes
            });
            self.alias("csrf".to_owned(), builder)?;
        }
        if !self.contains_name("web") {
            self.group(
                "web".to_owned(),
                vec!["cookies", "session", "csrf"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            )?;
        }
        if !self.contains_name("api") {
            // SubstituteBindings 已由受管理路由分发器执行；空追加链表示不重复包装。
            self.groups.insert("api".to_owned(), Vec::new());
        }
        Ok(())
    }

    pub(crate) fn install_framework_defaults(
        &mut self,
        signed_except: Vec<String>,
    ) -> crate::core::Result<()> {
        self.signed_except = signed_except;
        #[cfg(feature = "auth")]
        {
            for (name, guest) in [("auth", false), ("guest", true)] {
                if !self.contains_name(name) {
                    self.parameter_alias(
                        name.into(),
                        Arc::new(move |arguments| {
                            let options = crate::auth::middleware::guards(arguments, guest);
                            Ok(Arc::new(move |routes: Routes| {
                                routes.layer(axum::middleware::from_fn_with_state(
                                    options.clone(),
                                    crate::auth::middleware::handle,
                                ))
                            }))
                        }),
                    )?;
                }
            }
            if !self.contains_name("password.confirm") {
                self.parameter_alias(
                    "password.confirm".into(),
                    Arc::new(|arguments| {
                        let options = crate::auth::middleware::password(arguments)?;
                        Ok(Arc::new(move |routes: Routes| {
                            routes.layer(axum::middleware::from_fn_with_state(
                                options.clone(),
                                crate::auth::middleware::handle,
                            ))
                        }))
                    }),
                )?;
            }
        }

        if !self.contains_name("signed") {
            let options =
                super::middleware::ValidateSignature::new().except(self.signed_except.clone());
            self.alias(
                "signed".to_owned(),
                Arc::new(move |routes| {
                    routes.layer(axum::middleware::from_fn_with_state(
                        options.clone(),
                        super::middleware::validate_signature_with_options,
                    ))
                }),
            )?;
        }
        if !self.contains_name("precognitive") {
            self.alias(
                "precognitive".to_owned(),
                Arc::new(|routes| {
                    routes.layer(axum::middleware::from_fn(
                        super::precognition::handle_precognitive_requests,
                    ))
                }),
            )?;
        }
        Ok(())
    }
    pub(crate) fn priority(&mut self, names: Vec<String>) -> crate::core::Result<()> {
        if names.iter().any(|name| !valid_middleware_name(name)) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_invalid",
                "HTTP middleware names may contain only ASCII letters, digits, dots, underscores and hyphens",
            ));
        }
        self.priority = names;
        Ok(())
    }

    pub(crate) fn prepend_priority(&mut self, name: String) -> crate::core::Result<()> {
        if !valid_middleware_name(&name) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_invalid",
                "HTTP middleware names may contain only ASCII letters, digits, dots, underscores and hyphens",
            ));
        }
        if !self.priority.contains(&name) {
            self.priority.insert(0, name);
        }
        Ok(())
    }

    pub(crate) fn append_priority(&mut self, name: String) -> crate::core::Result<()> {
        if !valid_middleware_name(&name) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_invalid",
                "HTTP middleware names may contain only ASCII letters, digits, dots, underscores and hyphens",
            ));
        }
        if !self.priority.contains(&name) {
            self.priority.push(name);
        }
        Ok(())
    }
    pub(crate) fn alias(
        &mut self,
        name: String,
        builder: MiddlewareLayerBuilder,
    ) -> crate::core::Result<()> {
        self.validate_new_name(&name)?;
        self.aliases.insert(name, builder);
        Ok(())
    }

    pub(crate) fn parameter_alias(
        &mut self,
        name: String,
        factory: MiddlewareParameterFactory,
    ) -> crate::core::Result<()> {
        self.validate_new_name(&name)?;
        self.parameter_aliases.insert(name, factory);
        Ok(())
    }

    fn contains_name(&self, name: &str) -> bool {
        self.aliases.contains_key(name)
            || self.parameter_aliases.contains_key(name)
            || self.groups.contains_key(name)
    }

    pub(crate) fn group(&mut self, name: String, members: Vec<String>) -> crate::core::Result<()> {
        self.validate_new_name(&name)?;
        if members.is_empty() {
            return Err(crate::core::Error::custom(
                "http_middleware_group_empty",
                "HTTP middleware groups must contain at least one middleware name",
            ));
        }
        if members
            .iter()
            .any(|member| !valid_middleware_reference(member))
        {
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
        if self.contains_name(name) {
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
    ) -> crate::core::Result<Vec<(String, MiddlewareLayerBuilder)>> {
        if !valid_middleware_reference(name) {
            return Err(crate::core::Error::custom(
                "http_middleware_name_invalid",
                "HTTP middleware 引用名称或参数包含无效字符",
            ));
        }
        let (base, arguments) = match name.split_once(':') {
            Some((base, arguments)) => (base, arguments.split(',').map(str::to_owned).collect()),
            None => (name, Vec::new()),
        };
        if let Some(factory) = self.parameter_aliases.get(base) {
            return Ok(vec![(base.to_owned(), factory(arguments)?)]);
        }
        if let Some(alias) = self.aliases.get(name) {
            return Ok(vec![(name.to_owned(), Arc::clone(alias))]);
        }
        if let Some(arguments) = name.strip_prefix("signed:") {
            let mut arguments = arguments.split(',');
            let first = arguments.next().unwrap_or_default();
            let relative = first == "relative";
            let ignore = if relative {
                arguments.map(str::to_owned).collect::<Vec<_>>()
            } else {
                std::iter::once(first)
                    .chain(arguments)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            if ignore.iter().any(String::is_empty) || (!relative && first.is_empty()) {
                return Err(crate::core::Error::custom(
                    "http_middleware_name_invalid",
                    "signed middleware 参数不能为空",
                ));
            }
            let options = if relative {
                super::middleware::ValidateSignature::new()
                    .relative()
                    .ignoring(ignore)
            } else {
                super::middleware::ValidateSignature::new().ignoring(ignore)
            }
            .except(self.signed_except.clone());
            let builder: MiddlewareLayerBuilder = Arc::new(move |routes| {
                routes.layer(axum::middleware::from_fn_with_state(
                    options.clone(),
                    super::middleware::validate_signature_with_options,
                ))
            });
            return Ok(vec![("signed".to_owned(), builder)]);
        }
        let Some(members) = self.groups.get(name) else {
            return Err(crate::core::Error::custom(
                "http_middleware_name_unknown",
                format!("HTTP middleware alias or group `{base}` is not registered"),
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

fn valid_middleware_reference(name: &str) -> bool {
    match name.split_once(':') {
        Some((base, arguments)) => {
            valid_middleware_name(base) && !arguments.chars().any(char::is_control)
        }
        None => valid_middleware_name(name),
    }
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
    namespace: Option<String>,
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

    /// 设置控制器动作的登记命名空间；相对嵌套空间追加，前导反斜线重置。
    ///
    /// 仅作用于字符串动作声明，二元组保持绝对身份；与路由名称前缀无关。
    #[must_use]
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
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
/// 每个方法都会消费并返回 `Self`，使路由表可按从上到下的顺序阅读。
/// 最终声明同时派生逐方法分派与完整方法动作索引；相同原始集合键覆盖时保留首次位置。
/// 域表先于普通表合并，普通路径按声明顺序匹配，因此早声明的动态路径可以先于静态路径。
/// 全部普通候选均未命中后才尝试适用的 fallback；业务处理器返回的 `404`/`405` 是终态。
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
    last_route: Option<(String, String, Option<String>)>,
    last_metadata: Option<Arc<Mutex<MatchedRouteMetadata>>>,
    declarations: Vec<RouteDeclaration>,
    route_names: Vec<NamedRouteDefinition>,
    active_domain: Option<DomainPattern>,
    active_namespace: Option<String>,
    route_name_errors: Vec<String>,
    middleware_registry: Option<Arc<MiddlewareRegistry>>,
    active_middleware_name: Option<String>,
    middleware_errors: Vec<crate::core::Error>,
    route_errors: Vec<crate::core::Error>,
    fallback_registered: bool,
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

    /// 使用受管理的 [`RouteMethods`] 添加路由。
    #[must_use]
    pub fn route(mut self, path: &str, methods: RouteMethods) -> Self {
        for error in &methods.errors {
            self.route_errors.push(crate::core::Error::custom(
                "http_route_method_invalid",
                error.clone(),
            ));
        }
        let metadata = Arc::new(Mutex::new(MatchedRouteMetadata {
            name: None,
            action: None,
            path: path.to_owned(),
            domain: self
                .active_domain
                .as_ref()
                .map(|domain| domain.as_str().to_owned()),
            methods: methods
                .entries
                .iter()
                .flat_map(|entry| {
                    entry
                        .methods
                        .iter()
                        .map(|method| method.as_str().to_owned())
                })
                .collect(),
            runtime_prefix: String::new(),
            fallback: false,
        }));
        let (_, entries) = methods.materialize(&metadata);
        self.declarations.push(RouteDeclaration {
            metadata: Arc::clone(&metadata),
            entries,
        });
        self.last_metadata = Some(metadata);
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
            if let Some(metadata) = self.last_metadata.take()
                && let Ok(mut metadata) = metadata.lock()
            {
                metadata.name = Some(format!("{prefix}{name}"));
            }
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
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::get(handler))
    }

    /// 将动作身份与真实 GET/HEAD handler 同次登记。
    #[must_use]
    pub fn get_action<H, T>(
        self,
        path: &str,
        action: impl Into<crate::http::RouteAction>,
        handler: H,
    ) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.on_action(path, &["GET"], action, handler)
    }

    /// 将动作身份与指定方法的真实 handler 同次登记；方法规则与 [`Self::on`] 相同。
    #[must_use]
    pub fn on_action<H, T>(
        mut self,
        path: &str,
        methods: &[&str],
        action: impl Into<crate::http::RouteAction>,
        handler: H,
    ) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        let action = match action.into().registered(self.active_namespace.as_deref()) {
            Ok(action) => action,
            Err(error) => {
                self.route_errors.push(error);
                return self;
            }
        };
        self = self.on(path, methods, handler);
        if let Some(metadata) = &self.last_metadata {
            match metadata.lock() {
                Ok(mut metadata) => metadata.action = Some(action),
                Err(_) => self.route_errors.push(crate::core::Error::custom(
                    "http_route_action_invalid",
                    "动作路由元数据锁不可用",
                )),
            }
        }
        self
    }

    /// 注册 `POST` 路由。
    #[must_use]
    pub fn post<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::post(handler))
    }

    /// 注册 `PUT` 路由。
    #[must_use]
    pub fn put<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::put(handler))
    }

    /// 注册 `PATCH` 路由。
    #[must_use]
    pub fn patch<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::patch(handler))
    }

    /// 注册 `DELETE` 路由。
    #[must_use]
    pub fn delete<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::delete(handler))
    }

    /// 注册 `HEAD` 路由。
    #[must_use]
    pub fn head<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::head(handler))
    }

    /// 注册 `OPTIONS` 路由。
    #[must_use]
    pub fn options<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::options(handler))
    }

    /// 注册 `QUERY` 路由。
    #[must_use]
    pub fn query<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::query(handler))
    }

    /// 注册一个或多个合法 HTTP 方法，包括自定义方法。
    #[must_use]
    pub fn on<H, T>(self, path: &str, methods: &[&str], handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::on(methods, handler))
    }

    /// 注册固定 Laravel 八种方法。
    #[must_use]
    pub fn any<H, T>(self, path: &str, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState>,
        T: 'static,
    {
        self.route(path, routing::any(handler))
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
    pub fn group(mut self, group: RouteGroup, f: impl FnOnce(Routes) -> Routes) -> Self {
        let namespace = match crate::http::action::group_namespace(
            self.active_namespace.as_deref(),
            group.namespace.as_deref(),
        ) {
            Ok(namespace) => namespace,
            Err(error) => {
                self.route_errors.push(error);
                return self;
            }
        };
        let domain = match group.domain.as_deref() {
            Some(pattern) => match DomainPattern::parse(pattern) {
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
            active_domain: domain,
            active_namespace: namespace,
            ..Routes::new()
        };
        let prefix = group.prefix.clone();
        let name_prefix = group.name_prefix.clone();
        let mut inner = group.apply(f(child), self.middleware_registry.as_deref());
        for declaration in &inner.declarations {
            match declaration.metadata.lock() {
                Ok(mut metadata) => {
                    metadata.path = join_route_path(&prefix, &metadata.path);
                    metadata.runtime_prefix = format!("{prefix}{}", metadata.runtime_prefix);
                    metadata.name = metadata
                        .name
                        .take()
                        .map(|name| format!("{name_prefix}{name}"));
                }
                Err(_) => inner.route_errors.push(route_context_error()),
            }
        }
        for named in &mut inner.route_names {
            named.name = format!("{name_prefix}{}", named.name);
            named.path = join_route_path(&prefix, &named.path);
        }
        if let Some((path, pending_name_prefix, _)) = &mut inner.last_route {
            *path = join_route_path(&prefix, path);
            *pending_name_prefix = format!("{name_prefix}{pending_name_prefix}");
        }
        self.append_declarations(inner);
        self
    }

    /// 在同一层级合并另一张路由表。
    #[must_use]
    pub fn merge(mut self, mut other: Routes) -> Self {
        if let Some(pattern) = &self.active_domain {
            for named in &mut other.route_names {
                if named.domain.is_none() {
                    named.domain = Some(pattern.as_str().to_owned());
                }
            }
            for declaration in &other.declarations {
                match declaration.metadata.lock() {
                    Ok(mut metadata) if metadata.domain.is_none() => {
                        metadata.domain = Some(pattern.as_str().to_owned());
                    }
                    Ok(_) => {}
                    Err(_) => other.route_errors.push(route_context_error()),
                }
            }
            if let Some((_, _, domain)) = &mut other.last_route {
                *domain = domain.take().or_else(|| Some(pattern.as_str().to_owned()));
            }
        }
        self.append_declarations(other);
        self
    }

    fn append_declarations(&mut self, other: Routes) {
        self.declarations.extend(other.declarations);
        self.route_names.extend(other.route_names);
        self.route_name_errors.extend(other.route_name_errors);
        self.middleware_errors.extend(other.middleware_errors);
        self.route_errors.extend(other.route_errors);
        self.last_route = other.last_route;
        self.last_metadata = other.last_metadata;
    }

    /// 注册未匹配路由时执行的 fallback handler。
    ///
    /// GET/HEAD 请求在业务路由与根路径的自定义 `server.root_target` 之后，先尝试读取 `public`
    /// 目录中的静态资源，再调用 handler。
    /// 若用于 `RouteGroup` 内，fallback 只覆盖该组前缀，并遵循组的 middleware。
    /// 同方法普通路由优先；GET/HEAD 可接管其他方法路径的 miss。每个同层 router 只能注册一个 fallback；带路由参数或通配符的组前缀
    /// 暂不支持 fallback，构建时返回 `http_fallback_scope_unsupported`。
    #[must_use]
    pub fn fallback<H, T>(mut self, handler: H) -> Self
    where
        H: ManagedHandler<T, AppState> + Clone,
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
        let metadata = Arc::new(Mutex::new(MatchedRouteMetadata {
            name: None,
            action: None,
            path: "/{fallbackPlaceholder}".to_owned(),
            domain: self
                .active_domain
                .as_ref()
                .map(|domain| domain.as_str().to_owned()),
            methods: vec!["GET".to_owned(), "HEAD".to_owned()],
            runtime_prefix: String::new(),
            fallback: true,
        }));
        self.last_route = Some((
            "/{fallbackPlaceholder}".to_owned(),
            String::new(),
            self.active_domain
                .as_ref()
                .map(|domain| domain.as_str().to_owned()),
        ));
        self.last_metadata = Some(Arc::clone(&metadata));
        let (_, entries) = routing::get(handler).materialize(&metadata);
        self.declarations
            .push(RouteDeclaration { metadata, entries });
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
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        for declaration in &self.declarations {
            routing::layer_entries(
                &declaration.entries,
                layer.clone(),
                self.active_middleware_name.as_deref(),
            );
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

    pub(crate) fn into_parts(mut self) -> RouteParts {
        let priority = self
            .middleware_registry
            .as_deref()
            .map_or(&[][..], |registry| registry.priority.as_slice());
        for declaration in &self.declarations {
            routing::finalize_layers(&declaration.entries, priority);
        }
        let metadata: crate::core::Result<Vec<_>> = self
            .declarations
            .iter()
            .map(|declaration| snapshot_metadata(&declaration.metadata))
            .collect();
        let (actions, candidates, get_paths) = match metadata.and_then(|metadata| {
            validate_fallback_scopes(&metadata)?;
            Ok((
                final_action_routes(&metadata),
                compile_candidates(&self.declarations, &metadata)?,
                metadata
                    .iter()
                    .filter(|route| {
                        route.domain.is_none() && route.methods.iter().any(|method| method == "GET")
                    })
                    .map(|route| route.path.clone())
                    .collect(),
            ))
        }) {
            Ok(parts) => parts,
            Err(error) => {
                self.route_errors.push(error);
                (Vec::new(), Vec::new(), Vec::new())
            }
        };
        RouteParts {
            candidates,
            get_paths,
            names: self.route_names,
            actions,
            name_errors: self.route_name_errors,
            middleware_errors: self.middleware_errors,
            route_errors: self.route_errors,
        }
    }
}

fn route_context_error() -> crate::core::Error {
    crate::core::Error::custom("http_route_context_poisoned", "请求路由声明不可用")
}

// PHP 数组覆盖保留首个位置；域表先做左 union，之后才检查当前 host。
fn declaration_projection(metadata: &[MatchedRouteMetadata], method: Option<&str>) -> Vec<usize> {
    let mut positions = HashMap::new();
    let mut ordered = Vec::new();
    for (index, route) in metadata.iter().enumerate() {
        if let Some(method) = method
            && !route.methods.iter().any(|item| item == method)
        {
            continue;
        }
        let key = declaration_key(route, method.is_none());
        let bucket = (route.domain.is_some(), key);
        if let Some(&position) = positions.get(&bucket) {
            ordered[position] = index;
        } else {
            positions.insert(bucket, ordered.len());
            ordered.push(index);
        }
    }
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    for domain in [true, false] {
        for &index in &ordered {
            let route = &metadata[index];
            if route.domain.is_some() == domain
                && visited.insert(declaration_key(route, method.is_none()))
            {
                result.push(index);
            }
        }
    }
    result
}

fn declaration_key(route: &MatchedRouteMetadata, full: bool) -> String {
    let uri = route.path.trim_matches('/');
    format!(
        "{}{}{}",
        if full {
            route.methods.join("|")
        } else {
            String::new()
        },
        route.domain.as_deref().unwrap_or_default(),
        if uri.is_empty() { "/" } else { uri }
    )
}

fn validate_fallback_scopes(metadata: &[MatchedRouteMetadata]) -> crate::core::Result<()> {
    let mut scopes = HashSet::new();
    for route in metadata.iter().filter(|route| route.fallback) {
        let scope = fallback_scope_key(route.runtime_prefix.trim_end_matches('/'));
        if fallback_scope_unsupported(&scope) {
            return Err(crate::core::Error::custom(
                "http_fallback_scope_unsupported",
                "fallback routes cannot use parameterized or wildcard group prefixes",
            ));
        }
        if !scopes.insert((route.domain.clone(), scope)) {
            return Err(fallback_duplicate());
        }
    }
    Ok(())
}

fn compile_candidates(
    declarations: &[RouteDeclaration],
    metadata: &[MatchedRouteMetadata],
) -> crate::core::Result<Vec<DispatchCandidate>> {
    // matchit 内部以有限字母标记参数，第26项会panic；在调用Axum前转换成安全启动错误。
    for route in metadata {
        let mut chars = route.path.chars().peekable();
        let mut parameters = 0;
        while let Some(ch) = chars.next() {
            if ch == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                } else if chars.peek() != Some(&'*') {
                    parameters += 1;
                }
            }
        }
        if parameters > 25 {
            return Err(crate::core::Error::custom(
                "http_route_parameter_limit",
                "路由参数超过底层匹配器支持的上限",
            ));
        }
    }
    let methods: std::collections::BTreeSet<_> = metadata
        .iter()
        .flat_map(|route| route.methods.iter().cloned())
        .collect();
    let mut candidates = Vec::new();
    for method in methods {
        let parsed = Method::from_bytes(method.as_bytes()).map_err(|_| {
            crate::core::Error::custom("http_route_method_invalid", "invalid HTTP method token")
        })?;
        for index in declaration_projection(metadata, Some(&method)) {
            let route = &metadata[index];
            let entries = &declarations[index].entries;
            let scope = route.fallback.then(|| {
                let scope = route.runtime_prefix.trim_end_matches('/');
                if scope.is_empty() {
                    "/".to_owned()
                } else {
                    scope.to_owned()
                }
            });
            let router = if route.fallback {
                fallback_router(entries)
            } else {
                candidate(Router::new().route(&route.path, routing::materialize_shared(entries)))
            };
            let probe = candidate(Router::new().route(
                &route.path,
                axum::routing::any(|request: Request| async {
                    missed(request, InternalMiss::Method)
                }),
            ));
            candidates.push(DispatchCandidate {
                method: parsed.clone(),
                domain: route
                    .domain
                    .as_deref()
                    .map(DomainPattern::parse)
                    .transpose()?,
                router,
                probe,
                fallback_scope: scope,
            });
        }
    }
    Ok(candidates)
}

fn fallback_router(entries: &ManagedEntries) -> Router<AppState> {
    let entries = Arc::clone(entries);
    Router::new().fallback(move |State(state): State<AppState>, mut request: Request| {
        let entries = Arc::clone(&entries);
        async move {
            *request.uri_mut() = crate::http::site::routing_uri(&request).clone();
            if let Some(response) =
                crate::http::site::try_static_file(state.clone(), static_request(&request)).await
            {
                return response;
            }
            let entries = entries
                .lock()
                .map(|entries| entries.clone())
                .map_err(|_| route_context_error());
            match entries {
                Ok(entries) => dispatch_managed(request, state, &entries).await,
                Err(error) => error.into_response(),
            }
        }
    })
}

fn final_action_routes(metadata: &[MatchedRouteMetadata]) -> Vec<ActionRouteDefinition> {
    declaration_projection(metadata, None)
        .into_iter()
        .filter_map(|index| {
            let route = &metadata[index];
            route
                .action
                .as_ref()
                .filter(|action| action.as_str() != "0")
                .map(|action| ActionRouteDefinition {
                    action: action.clone(),
                    path: route.path.clone(),
                    domain: route.domain.clone(),
                })
        })
        .collect()
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
            for (alias, builder) in builders.into_iter().rev() {
                routes.active_middleware_name = Some(alias);
                routes = builder(routes);
                routes.active_middleware_name = None;
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
    let trailing_slash = path != "/" && path.ends_with('/');
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

/// 供站点层取回业务未匹配的根请求，或全部用户 fallback 均未匹配的其他请求。
pub(crate) fn take_unmatched(response: &mut Response) -> Option<Request> {
    response
        .extensions_mut()
        .remove::<UnmatchedRequest>()?
        .0
        .lock()
        .ok()?
        .take()
}

fn unmatched(request: Request) -> Response {
    let slot = UnmatchedRequest(Arc::new(Mutex::new(Some(request))));
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NOT_FOUND;
    response.extensions_mut().insert(slot);
    response
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

/// 候选各自使用 Axum 解析路径；集合顺序决定先后，业务响应不以状态码回退。
pub(crate) fn assemble(parts: RouteParts) -> Router<AppState> {
    let candidates = Arc::new(parts.candidates);
    Router::new()
        .fallback(move |State(state): State<AppState>, request: Request| {
            let candidates = Arc::clone(&candidates);
            async move { dispatch_collection(&candidates, state, request).await }
        })
        .layer(axum::middleware::from_fn(
            |mut request: Request, next: axum::middleware::Next| async move {
                if request
                    .extensions()
                    .get::<crate::http::route::RouteSlot>()
                    .is_none()
                {
                    request
                        .extensions_mut()
                        .insert(crate::http::route::RouteSlot::default());
                }
                next.run(request).await
            },
        ))
}

#[allow(
    clippy::too_many_lines,
    reason = "单次路由决策同时维护方法、域名与 fallback 优先顺序"
)]
async fn dispatch_collection(
    candidates: &[DispatchCandidate],
    state: AppState,
    request: Request,
) -> Response {
    let host = if candidates
        .iter()
        .any(|candidate| candidate.domain.is_some())
    {
        let host = request
            .extensions()
            .get::<super::proxy::ProxySnapshot>()
            .cloned()
            .unwrap_or_else(|| {
                super::proxy::ProxySnapshot::capture(
                    request.headers(),
                    request.uri(),
                    request.extensions(),
                    Some(&state.config().server),
                )
            })
            .context()
            .and_then(|origin| {
                origin.host()?.map_or_else(
                    || authority(request.headers(), request.uri()),
                    |(host, _)| Ok(host),
                )
            });
        match host.and_then(|host| {
            request
                .extensions()
                .get::<super::trusted_hosts::TrustedHostsSnapshot>()
                .cloned()
                .unwrap_or_default()
                .check(&host)?;
            Ok(host)
        }) {
            Ok(host) => Some(host),
            Err(error) => return error.into_response(),
        }
    } else {
        None
    };
    let snapshot = RequestSnapshot::capture(&request);
    let mut pending = match run_collection_candidates(
        candidates,
        false,
        host.as_deref(),
        &snapshot,
        state.clone(),
        request,
    )
    .await
    {
        Ok(request) => request,
        Err(response) => return response,
    };
    let (request, mut allowed) = match alternate_methods(
        candidates,
        false,
        host.as_deref(),
        &snapshot,
        state.clone(),
        pending,
    )
    .await
    {
        Ok(result) => result,
        Err(response) => return response,
    };
    pending = request;
    // 显式站点根目标只接管无普通路径方法候选的根 GET/HEAD；消费标记防止 forward 重入。
    let root_target = pending
        .extensions_mut()
        .remove::<crate::http::site::PendingRootTarget>()
        .is_some();
    if root_target
        && allowed.is_empty()
        && snapshot.uri.path() == "/"
        && matches!(snapshot.method, Method::GET | Method::HEAD)
    {
        snapshot.restore(&mut pending);
        pending
            .extensions_mut()
            .remove::<crate::http::site::PendingRootTarget>();
        return unmatched(pending);
    }
    pending = match run_collection_candidates(
        candidates,
        true,
        host.as_deref(),
        &snapshot,
        state.clone(),
        pending,
    )
    .await
    {
        Ok(request) => request,
        Err(response) => return response,
    };
    let (mut pending, fallback_allowed) =
        match alternate_methods(candidates, true, host.as_deref(), &snapshot, state, pending).await
        {
            Ok(result) => result,
            Err(response) => return response,
        };
    allowed.extend(fallback_allowed);
    if allowed.is_empty() {
        snapshot.restore(&mut pending);
        return unmatched(pending);
    }
    method_response(&pending, &allowed)
}

// 同域静态 scope 的深层优先是既有 Rust 组 fallback 适配；各域模式与普通表仍按集合顺序。
fn ordered_candidates<'a>(
    candidates: &'a [DispatchCandidate],
    method: &Method,
    fallback: bool,
) -> Vec<&'a DispatchCandidate> {
    let mut selected: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            candidate.method == *method && candidate.fallback_scope.is_some() == fallback
        })
        .collect();
    if fallback {
        let mut patterns = HashMap::new();
        for (index, candidate) in selected.iter().enumerate() {
            patterns
                .entry(candidate.domain.as_ref().map(DomainPattern::as_str))
                .or_insert(index);
        }
        selected.sort_by_key(|candidate| {
            (
                patterns[&candidate.domain.as_ref().map(DomainPattern::as_str)],
                std::cmp::Reverse(candidate.fallback_scope.as_ref().map_or(0, String::len)),
            )
        });
    }
    selected
}

fn prepare_candidate(
    candidate: &DispatchCandidate,
    host: Option<&str>,
    snapshot: &RequestSnapshot,
    request: &mut Request,
) -> bool {
    snapshot.restore(request);
    if let Some(pattern) = &candidate.domain {
        let Some(values) = host.and_then(|host| pattern.captures(host)) else {
            return false;
        };
        crate::http::domain::set_domain_params(request.extensions_mut(), values);
    }
    candidate
        .fallback_scope
        .as_ref()
        .is_none_or(|scope| scope_matches(scope, snapshot.uri.path()))
}

#[expect(
    clippy::result_large_err,
    reason = "终态响应原样传递，与既有 run_candidate 保持一致"
)]
async fn run_collection_candidates(
    candidates: &[DispatchCandidate],
    fallback: bool,
    host: Option<&str>,
    snapshot: &RequestSnapshot,
    state: AppState,
    mut pending: Request,
) -> std::result::Result<Request, Response> {
    for candidate in ordered_candidates(candidates, &snapshot.method, fallback) {
        if !prepare_candidate(candidate, host, snapshot, &mut pending) {
            continue;
        }
        match run_candidate(&candidate.router, state.clone(), pending).await {
            Ok((request, _)) => pending = request,
            Err(response) => return Err(response),
        }
    }
    snapshot.restore(&mut pending);
    Ok(pending)
}

#[expect(
    clippy::result_large_err,
    reason = "路径探测错误原样终止，不执行 handler 或复制请求 body"
)]
async fn alternate_methods(
    candidates: &[DispatchCandidate],
    fallback: bool,
    host: Option<&str>,
    snapshot: &RequestSnapshot,
    state: AppState,
    mut pending: Request,
) -> std::result::Result<(Request, std::collections::BTreeSet<String>), Response> {
    let mut allowed = std::collections::BTreeSet::new();
    for method in OFFICIAL_METHODS
        .into_iter()
        .filter(|method| *method != snapshot.method.as_str())
    {
        let Some(parsed) = Method::from_bytes(method.as_bytes()).ok() else {
            continue;
        };
        for candidate in ordered_candidates(candidates, &parsed, fallback) {
            if !prepare_candidate(candidate, host, snapshot, &mut pending) {
                continue;
            }
            if fallback {
                allowed.insert(method.to_owned());
                break;
            }
            match run_candidate(&candidate.probe, state.clone(), pending).await {
                Ok((request, matched)) => {
                    pending = request;
                    if matched.is_some() {
                        allowed.insert(method.to_owned());
                        break;
                    }
                }
                Err(response) => return Err(response),
            }
        }
    }
    snapshot.restore(&mut pending);
    Ok((pending, allowed))
}

fn method_response(request: &Request, allowed: &std::collections::BTreeSet<String>) -> Response {
    let options = *request.method() == Method::OPTIONS;
    let mut response = if options {
        Response::new(Body::empty())
    } else {
        crate::core::Error::custom_with_status(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "method not allowed",
        )
        .into_response()
    };
    if let Ok(value) = axum::http::HeaderValue::from_str(&allow_header(allowed, options)) {
        response.headers_mut().insert(ALLOW, value);
    }
    if options && let Some(slot) = request.extensions().get::<crate::http::route::RouteSlot>() {
        let uri = request.uri().path().trim_matches('/');
        if let Err(error) = slot.publish(crate::http::Route::new(
            None,
            if uri.is_empty() { "/" } else { uri },
            None,
            vec!["OPTIONS".to_owned()],
            Vec::new(),
        )) {
            return error.into_response();
        }
    }
    response
}

fn scope_matches(scope: &str, path: &str) -> bool {
    let scope = scope.trim_end_matches('/');
    scope.is_empty() || path == scope || path.starts_with(&format!("{scope}/"))
}

fn allow_header(allowed: &std::collections::BTreeSet<String>, options: bool) -> String {
    OFFICIAL_METHODS
        .into_iter()
        .filter(|method| allowed.contains(*method))
        .collect::<Vec<_>>()
        .join(if options { "," } else { ", " })
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
    if let Some(slot) = slot.0.lock().ok().and_then(|guard| {
        guard.as_ref().and_then(|request| {
            request
                .extensions()
                .get::<crate::http::route::RouteSlot>()
                .cloned()
        })
    }) && let Err(error) = slot.clear()
    {
        return Err(error.into_response());
    }
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

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::core::Result;
    use crate::http::RouteUrlGenerator;
    use std::collections::BTreeMap;

    async fn handler() -> &'static str {
        "actual handler"
    }

    fn urls(routes: Routes) -> Result<RouteUrlGenerator> {
        let parts = routes.into_parts();
        assert!(parts.route_errors.is_empty());
        RouteUrlGenerator::new_with_actions("https://configured.test", parts.names, parts.actions)
    }

    #[test]
    fn action_collection_composite_keys_preserve_php_refresh_overwrite_rules() -> Result<()> {
        // 相同完整方法序列才替换；拆分完全覆盖仍保留上游 allRoutes 的旧组合键。
        let generator = urls(
            Routes::new()
                .on_action("/same", &["GET", "POST"], "Old@go", handler)
                .on_action("/same", &["GET", "POST"], "New@go", handler)
                .on_action("/split", &["GET", "POST"], "SplitOld@go", handler)
                .get_action("/split", "SplitNew@get", handler)
                .on_action("/split", &["POST"], "SplitNew@post", handler)
                .on_action("/order", &["GET", "POST"], "OrderOld@go", handler)
                .on_action("/order", &["POST", "GET"], "OrderNew@go", handler)
                .on_action("/partial", &["GET", "POST"], "PartialOld@go", handler)
                .get_action("/partial", "PartialNew@get", handler)
                .get_action("/anonymous", "Removed@go", handler)
                .get("/anonymous", handler),
        )?;
        let parameters = BTreeMap::new();
        for action in ["Old@go", "Removed@go"] {
            assert_eq!(
                generator
                    .action(action, &parameters)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("http_route_action_not_found")
            );
        }
        for (action, path) in [
            ("New@go", "same"),
            ("SplitOld@go", "split"),
            ("SplitNew@get", "split"),
            ("SplitNew@post", "split"),
            ("OrderOld@go", "order"),
            ("OrderNew@go", "order"),
            ("PartialOld@go", "partial"),
            ("PartialNew@get", "partial"),
        ] {
            assert_eq!(
                generator.action(action, &parameters)?,
                format!("https://configured.test/{path}")
            );
        }
        Ok(())
    }

    #[test]
    fn action_collection_domain_priority_first_key_and_backslash_asymmetry() -> Result<()> {
        // 声明原值检查存在、规范key写入，后续带反斜线的动作可以覆盖。
        let generator = urls(
            Routes::new()
                .get_action("/plain", "Shared@go", handler)
                .group(RouteGroup::new().domain("tenant.test"), |routes| {
                    routes.get_action("/domain", "Shared@go", handler)
                })
                .get_action("/first", "First@go", handler)
                .get_action("/second", "First@go", handler)
                .get_action("/slash-first", "Slash@go", handler)
                .get_action("/slash-second", "\\Slash@go", handler)
                .get_action("/slash-third", "Slash@go", handler),
        )?;
        for (action, expected) in [
            ("Shared@go", "https://tenant.test/domain"),
            ("First@go", "https://configured.test/first"),
            ("Slash@go", "https://configured.test/slash-second"),
        ] {
            assert_eq!(generator.action(action, &BTreeMap::new())?, expected);
        }
        Ok(())
    }

    #[test]
    fn action_collection_uses_raw_concatenated_key_and_domain_left_union() -> Result<()> {
        // PHP 的 domain+uri 无分隔拼接会碰撞；不得改成各字段组成的安全元组。
        let generator = urls(
            Routes::new()
                .group(RouteGroup::new().domain("a.test"), |routes| {
                    routes.get_action("/bc", "Old@go", handler)
                })
                .group(RouteGroup::new().domain("a.testb"), |routes| {
                    routes.get_action("/c", "New@go", handler)
                }),
        )?;
        assert_eq!(
            generator
                .action("Old@go", &BTreeMap::new())
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        assert_eq!(
            generator.action("New@go", &BTreeMap::new())?,
            "https://a.testb/c"
        );
        let generator = urls(
            Routes::new()
                .group(RouteGroup::new().domain("a.test"), |routes| {
                    routes.get_action("/bc", "Domain@go", handler)
                })
                .get_action("/a.testbc", "Plain@go", handler),
        )?;
        assert_eq!(
            generator.action("Domain@go", &BTreeMap::new())?,
            "https://a.test/bc"
        );
        assert_eq!(
            generator
                .action("Plain@go", &BTreeMap::new())
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        Ok(())
    }

    #[test]
    fn action_groups_merge_real_metadata_and_tuple_bypasses_namespace() -> Result<()> {
        let routes = Routes::new().group(
            RouteGroup::new()
                .namespace("App")
                .prefix("/app")
                .name_prefix("app."),
            |routes| {
                routes.group(
                    RouteGroup::new().namespace("Admin").prefix("/admin"),
                    |routes| {
                        routes
                            .get_action("/draft", "Draft@show", handler)
                            .name("draft")
                            .get_action("/similar", "App\\AdminExtra@show", handler)
                            .get_action("/tuple", ("External", "show"), handler)
                            .group(RouteGroup::new().namespace("\\Other"), |routes| {
                                routes.get_action("/reset", "Invokable", handler)
                            })
                    },
                )
            },
        );
        let metadata: Vec<_> = routes
            .declarations
            .iter()
            .map(|declaration| snapshot_metadata(&declaration.metadata))
            .collect::<Result<_>>()?;
        assert_eq!(
            metadata[0].action.as_deref(),
            Some("App\\Admin\\Draft@show")
        );
        assert_eq!(metadata[0].name.as_deref(), Some("app.draft"));
        assert_eq!(metadata[0].path, "/app/admin/draft");
        assert_eq!(metadata[0].methods, ["GET", "HEAD"]);
        assert_eq!(metadata[1].action.as_deref(), Some("App\\AdminExtra@show"));
        assert_eq!(metadata[2].action.as_deref(), Some("External@show"));
        assert_eq!(metadata[3].action.as_deref(), Some("Other\\Invokable"));
        let generator = urls(routes)?;
        assert_eq!(
            generator.action(("External", "show"), &BTreeMap::new())?,
            "https://configured.test/app/admin/tuple"
        );
        assert_eq!(
            generator.route("app.draft", &BTreeMap::new())?,
            "https://configured.test/app/admin/draft"
        );
        Ok(())
    }

    #[test]
    fn invalid_action_registration_and_namespace_accumulate_startup_errors() {
        for routes in [
            Routes::new().get_action("/bad", "", handler),
            Routes::new().group(RouteGroup::new().namespace("App\n"), |routes| {
                routes.get_action("/bad", "Draft@show", handler)
            }),
        ] {
            let parts = routes.into_parts();
            assert_eq!(parts.route_errors[0].code(), "http_route_action_invalid");
            assert!(parts.actions.is_empty());
        }
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;
    use crate::core::Result;

    async fn handler() -> &'static str {
        "声明处理器"
    }

    fn metadata(routes: &Routes) -> Result<Vec<MatchedRouteMetadata>> {
        routes
            .declarations
            .iter()
            .map(|declaration| snapshot_metadata(&declaration.metadata))
            .collect()
    }

    #[test]
    fn collection_projection_raw_collision_and_domain_union_apply_before_host() -> Result<()> {
        // 匿名项也会被拼接碰撞淘汰；不能按 host 过滤后复活普通项。
        let routes = Routes::new()
            .get("/a.testbc", handler)
            .group(RouteGroup::new().domain("a.test"), |routes| {
                routes.get("/bc", handler)
            })
            .group(RouteGroup::new().domain("a.testb"), |routes| {
                routes.get("/c", handler)
            })
            .post("/a.testbc", handler);
        let metadata = metadata(&routes)?;
        assert_eq!(declaration_projection(&metadata, Some("GET")), [2]);
        assert_eq!(declaration_projection(&metadata, Some("HEAD")), [2]);
        assert_eq!(declaration_projection(&metadata, Some("POST")), [3]);
        assert_eq!(declaration_projection(&metadata, None), [2, 3]);
        let parts = routes.into_parts();
        assert!(parts.route_errors.is_empty());
        assert!(parts.actions.is_empty());
        assert_eq!(parts.candidates.len(), 3);
        Ok(())
    }

    #[test]
    fn collection_projection_split_and_partial_keep_original_metadata_and_shared_entries()
    -> Result<()> {
        // 方法 view 淘汰处理资格，完整组合 view 仍保留旧动作；不改命中 Route 原 methods。
        let routes = Routes::new()
            .on_action("/split", &["GET", "POST"], "Old@go", handler)
            .get_action("/split", "New@get", handler)
            .on_action("/split", &["POST"], "New@post", handler)
            .on_action("/partial", &["GET", "POST"], "Partial@go", handler)
            .get_action("/partial", "Partial@get", handler);
        let metadata = metadata(&routes)?;
        assert_eq!(declaration_projection(&metadata, Some("GET")), [1, 4]);
        assert_eq!(declaration_projection(&metadata, Some("POST")), [2, 3]);
        assert_eq!(declaration_projection(&metadata, None), [0, 1, 2, 3, 4]);
        assert_eq!(metadata[3].methods, ["GET", "POST", "HEAD"]);
        assert_eq!(metadata[3].action.as_deref(), Some("Partial@go"));
        for declaration in &routes.declarations {
            let entries = declaration
                .entries
                .lock()
                .map_err(|_| route_context_error())?;
            assert!(entries.iter().all(|entry| {
                entry
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| Arc::ptr_eq(metadata, &declaration.metadata))
            }));
        }
        Ok(())
    }

    #[test]
    fn collection_replacement_keeps_first_position_without_static_path_reordering() -> Result<()> {
        // 同 key 更新只换值；动态/静态路径树优先级不得重新排列候选。
        let routes = Routes::new()
            .get("/items/{id}", handler)
            .get("/items/fixed", handler)
            .get("/items/{id}", handler)
            .group(RouteGroup::new().domain("{account}.test"), |routes| {
                routes.get("/items/{id}", handler)
            });
        let metadata = metadata(&routes)?;
        assert_eq!(declaration_projection(&metadata, Some("GET")), [3, 2, 1]);
        let parts = routes.into_parts();
        assert!(parts.route_errors.is_empty());
        let candidates = ordered_candidates(&parts.candidates, &Method::GET, false);
        assert_eq!(candidates.len(), 3);
        assert_eq!(
            candidates[0].domain.as_ref().map(DomainPattern::as_str),
            Some("{account}.test")
        );
        assert!(candidates[1].domain.is_none());
        Ok(())
    }

    #[test]
    fn collection_fallback_uses_same_projection_and_scopes_keep_existing_adapter() -> Result<()> {
        // fallback 不是绕开集合的独立处理器；被同 key 普通项覆盖后不能复活。
        let routes = Routes::new()
            .fallback(handler)
            .get("/{fallbackPlaceholder}", handler);
        let metadata = metadata(&routes)?;
        assert_eq!(declaration_projection(&metadata, Some("GET")), [1]);
        let parts = routes.into_parts();
        assert!(parts.route_errors.is_empty());
        assert!(ordered_candidates(&parts.candidates, &Method::GET, true).is_empty());
        let parts = Routes::new()
            .fallback(handler)
            .group(RouteGroup::new().prefix("/api"), |routes| {
                routes.fallback(handler)
            })
            .group(RouteGroup::new().prefix("/api/v2"), |routes| {
                routes.fallback(handler)
            })
            .into_parts();
        assert!(parts.route_errors.is_empty());
        let scopes: Vec<_> = ordered_candidates(&parts.candidates, &Method::GET, true)
            .iter()
            .filter_map(|candidate| candidate.fallback_scope.as_deref())
            .collect();
        assert_eq!(scopes, ["/api/v2", "/api", "/"]);
        Ok(())
    }

    #[test]
    fn collection_group_merge_updates_single_source_and_rejects_invalid_fallback_scopes()
    -> Result<()> {
        // 组路径、域与名字只更新共享声明，参数化 fallback 的原限制仍在构建时报告。
        let routes = Routes::new().group(
            RouteGroup::new()
                .prefix("/api/{version}")
                .domain("{account}.test")
                .name_prefix("api."),
            |routes| {
                routes.merge(
                    Routes::new()
                        .get_action("/items/{id}", "Items@show", handler)
                        .name("items"),
                )
            },
        );
        let metadata = metadata(&routes)?;
        assert_eq!(metadata[0].path, "/api/{version}/items/{id}");
        assert_eq!(metadata[0].runtime_prefix, "/api/{version}");
        assert_eq!(metadata[0].domain.as_deref(), Some("{account}.test"));
        assert_eq!(metadata[0].name.as_deref(), Some("api.items"));
        let parts = routes.into_parts();
        assert!(parts.route_errors.is_empty());
        assert_eq!(parts.names[0].path, parts.actions[0].path);
        let parts = Routes::new()
            .group(RouteGroup::new().prefix("/api/{version}"), |routes| {
                routes.fallback(handler)
            })
            .into_parts();
        assert_eq!(
            parts.route_errors.first().map(crate::core::Error::code),
            Some("http_fallback_scope_unsupported")
        );
        let parts = Routes::new()
            .fallback(handler)
            .merge(Routes::new().fallback(handler))
            .into_parts();
        assert_eq!(
            parts.route_errors.first().map(crate::core::Error::code),
            Some("http_fallback_duplicate")
        );
        Ok(())
    }

    #[test]
    fn collection_allow_uses_fixed_verbs_and_preserves_two_wire_formats() {
        // QUERY 保留，TRACE/CONNECT 只允许显式命中；405 与合成 OPTIONS 的空格不同。
        let methods = ["QUERY", "TRACE", "CONNECT", "PATCH", "POST"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(allow_header(&methods, false), "POST, PATCH, QUERY");
        assert_eq!(allow_header(&methods, true), "POST,PATCH,QUERY");
        assert_eq!(
            OFFICIAL_METHODS,
            [
                "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY"
            ]
        );
    }

    #[test]
    fn collection_group_runtime_path_keeps_nested_parameter_and_encoded_segments() {
        // 独立路径候选不能丢失原 Axum nest 对 handler 可见 URI 的裁剪行为。
        for (path, prefix, expected) in [
            ("/api/v1/items/a%2Fb", "/api/{version}", "/items/a%2Fb"),
            ("/api/v1", "/api/{version}", "/"),
            ("/api/v1/", "/api/{version}", "/"),
            ("/items/42", "", "/items/42"),
            ("/api/v1/extra/items", "/api/{version}/extra", "/items"),
        ] {
            assert_eq!(route_runtime_path(path, prefix), expected);
        }
    }
}

#[cfg(test)]
mod parameter_budget_tests {
    use super::*;

    #[test]
    fn final_route_parameter_budget_preserves_catch_all_and_rejects_overflow() -> crate::Result<()>
    {
        use std::fmt::Write as _;
        let mut prefix = String::new();
        for n in 0..25 {
            write!(&mut prefix, "/{{p{n}}}").map_err(crate::Error::internal)?;
        }
        let valid = Routes::new().get(&format!("{prefix}/{{*rest}}"), || async { "ok" });
        let _ = valid.into_router()?;
        let invalid = Routes::new().group(RouteGroup::new().prefix(&prefix), |routes| {
            routes.get("/{child}", || async { "unreachable" })
        });
        assert!(
            matches!(invalid.into_router(), Err(error) if error.code() == "http_route_parameter_limit")
        );
        Ok(())
    }
}

#[cfg(test)]
mod parameter_alias_tests {
    use super::*;
    use crate::Result;

    fn identity() -> MiddlewareLayerBuilder {
        Arc::new(|routes| routes)
    }

    #[test]
    fn parameters_keep_empty_entries_and_alias_metadata_uses_base_name() -> Result<()> {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let calls = observed.clone();
        let mut registry = MiddlewareRegistry::default();
        registry.parameter_alias(
            "auth".into(),
            Arc::new(move |arguments| {
                calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(arguments);
                Ok(identity())
            }),
        )?;
        for reference in ["auth", "auth:", "auth:staff,,api"] {
            let expanded = registry.expand(reference, &mut HashSet::new())?;
            assert_eq!(expanded.len(), 1);
            assert_eq!(expanded[0].0, "auth");
        }
        assert_eq!(
            *observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                Vec::<String>::new(),
                vec![String::new()],
                vec!["staff".into(), String::new(), "api".into()]
            ]
        );
        registry.group("protected".into(), vec!["auth:staff".into()])?;
        registry.validate()?;
        Ok(())
    }

    #[test]
    fn duplicate_names_and_invalid_references_are_rejected_before_factory() -> Result<()> {
        let mut registry = MiddlewareRegistry::default();
        registry.parameter_alias(
            "auth".into(),
            Arc::new(|_| Err(crate::Error::custom("invalid_guard", "未知Guard"))),
        )?;
        assert!(registry.alias("auth".into(), identity()).is_err());
        assert!(registry.group("auth".into(), vec!["auth".into()]).is_err());
        assert!(
            registry
                .parameter_alias("auth".into(), Arc::new(|_| Ok(identity())))
                .is_err()
        );
        assert!(
            registry
                .parameter_alias("bad:name".into(), Arc::new(|_| Ok(identity())))
                .is_err()
        );
        for name in ["auth:staff", "auth:", "auth"] {
            assert!(registry.expand(name, &mut HashSet::new()).is_err());
        }
        for name in [":staff", "bad name:staff", "auth:staff\n", "auth:\u{0085}"] {
            assert!(!valid_middleware_reference(name));
            assert!(registry.expand(name, &mut HashSet::new()).is_err());
        }
        let mut static_registry = MiddlewareRegistry::default();
        static_registry.alias("auth".into(), identity())?;
        assert!(
            static_registry
                .parameter_alias("auth".into(), Arc::new(|_| Ok(identity())))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn framework_and_web_defaults_respect_parameter_aliases_and_keep_signed_support() -> Result<()>
    {
        let mut registry = MiddlewareRegistry::default();
        for name in [
            "cookies",
            "session",
            "csrf",
            "web",
            "api",
            "signed",
            "precognitive",
        ] {
            registry.parameter_alias(name.into(), Arc::new(|_| Ok(identity())))?;
        }
        registry.install_framework_defaults(vec!["tracking".into()])?;
        #[cfg(feature = "session")]
        registry.install_web_defaults(None, None, None)?;
        #[cfg(not(feature = "session"))]
        registry.install_web_defaults(None)?;
        let expected = [
            "cookies",
            "session",
            "csrf",
            "web",
            "api",
            "signed",
            "precognitive",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
        #[cfg(feature = "auth")]
        let expected = expected
            .into_iter()
            .chain(
                ["auth", "guest", "password.confirm"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            registry
                .parameter_aliases
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        assert!(registry.aliases.is_empty());
        assert!(registry.groups.is_empty());
        assert!(
            registry
                .expand("signed:custom", &mut HashSet::new())
                .is_ok()
        );
        let mut defaults = MiddlewareRegistry::default();
        defaults.install_framework_defaults(vec!["tracking".into()])?;
        assert!(defaults.expand("signed", &mut HashSet::new()).is_ok());
        assert!(
            defaults
                .expand("signed:relative,tracking", &mut HashSet::new())
                .is_ok()
        );
        assert!(defaults.expand("signed:", &mut HashSet::new()).is_err());
        #[cfg(feature = "session")]
        defaults.install_web_defaults(None, None, None)?;
        #[cfg(not(feature = "session"))]
        defaults.install_web_defaults(None)?;
        assert_eq!(
            defaults.groups.get("web"),
            Some(&vec![
                "cookies".to_owned(),
                "session".to_owned(),
                "csrf".to_owned()
            ])
        );
        assert_eq!(defaults.groups.get("api"), Some(&Vec::<String>::new()));
        Ok(())
    }
}
