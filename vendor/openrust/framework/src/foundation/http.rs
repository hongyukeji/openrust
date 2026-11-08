//! 应用HTTP装配、请求作用域、URL快照和异常收尾。

use crate::container::{ParameterValue, Resolver, Scope};
use crate::core::{Error, Result};
use crate::foundation::exceptions::ExceptionRegistry;
use crate::foundation::{
    Application, ApplicationState, BootContext, ExceptionHandler, ExceptionRequest,
};
use crate::http::authority::authority;
use crate::http::client_ip::validate_trusted_proxy_ranges;
#[cfg(test)]
use crate::http::server_provider::HttpServiceProvider;
use crate::http::trusted_hosts::TrustedHostsSnapshot;
use crate::http::{RequestAttributes, health, middleware};
use crate::routing::route::RouteSlot;
use crate::routing::router::assemble;
use crate::routing::{RouteRegistry, Router, UrlGenerator};
use axum::Router as AxumRouter;
use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::Route as AxumRoute;
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};

tokio::task_local! {
    // 只有 HTTP 入口能安装它。公开 Scope::run 的容器上下文可由应用自行替换。
    static HTTP_ENTRY_SCOPE: Arc<HttpEntry>;
}

struct HttpEntry {
    scope: Arc<Scope>,
    attributes: RequestAttributes,
    route: RouteSlot,
}

pub(crate) fn http_entry_scope() -> Option<std::sync::Weak<Scope>> {
    HTTP_ENTRY_SCOPE
        .try_with(|entry| Arc::downgrade(&entry.scope))
        .ok()
}

pub(crate) fn current_request_attribute(
    resolver: &Resolver,
    name: &str,
) -> Result<Option<ParameterValue>> {
    HTTP_ENTRY_SCOPE
        .try_with(|entry| {
            if resolver.belongs_to_http_scope(&entry.scope) {
                entry.attributes.get(name)
            } else {
                Ok(None)
            }
        })
        .unwrap_or(Ok(None))
}

pub(crate) fn current_route_parameter(
    resolver: &Resolver,
    name: &str,
) -> Result<Option<ParameterValue>> {
    HTTP_ENTRY_SCOPE
        .try_with(|entry| {
            if resolver.belongs_to_http_scope(&entry.scope) {
                Ok(entry
                    .route
                    .route()?
                    .and_then(|route| route.bound_parameter(name)))
            } else {
                Ok(None)
            }
        })
        .unwrap_or(Ok(None))
}

type RouterLayerBuilder = Box<dyn Fn(AxumRouter) -> Result<AxumRouter> + Send + Sync>;

#[derive(Default)]
struct RouterLayerRegistry {
    builders: Vec<RouterLayerBuilder>,
}

type TrustedHostsCallback = Arc<
    dyn Fn(ApplicationState) -> crate::support::BoxFuture<'static, Result<Vec<String>>>
        + Send
        + Sync,
>;

#[derive(Clone)]
enum TrustedHostsSource {
    Static(crate::config::TrustedHostsConfig),
    Dynamic {
        callback: TrustedHostsCallback,
        subdomains: bool,
    },
}

#[derive(Default)]
struct TrustedHostsRegistry(Option<TrustedHostsSource>);

#[derive(Clone)]
struct TrustedHostsRuntime {
    app: ApplicationState,
    source: Option<TrustedHostsSource>,
}

impl TrustedHostsRuntime {
    async fn snapshot(&self) -> Result<TrustedHostsSnapshot> {
        let app_url = &self.app.config().app.url;
        match &self.source {
            None => Ok(TrustedHostsSnapshot::disabled()),
            Some(TrustedHostsSource::Static(config)) => {
                Ok(TrustedHostsSnapshot::from_config(config, app_url))
            }
            Some(TrustedHostsSource::Dynamic {
                callback,
                subdomains,
            }) => {
                let patterns = callback(self.app.clone()).await.map_err(Error::internal)?;
                Ok(TrustedHostsSnapshot::from_patterns(
                    Some(patterns),
                    *subdomains,
                    app_url,
                ))
            }
        }
    }
}

impl BootContext<'_> {
    /// 为当前应用设置静态可信主机模式，覆盖 `[server.trusted_hosts]`。
    pub fn trust_hosts(&mut self, config: crate::config::TrustedHostsConfig) -> Result<&mut Self> {
        self.extension_mut::<TrustedHostsRegistry>()?.0 = Some(TrustedHostsSource::Static(config));
        Ok(self)
    }

    /// 每个请求求值一次公开 PCRE 模式；后登记的策略覆盖先登记者。
    pub fn trust_hosts_with<F>(&mut self, subdomains: bool, callback: F) -> Result<&mut Self>
    where
        F: Fn(ApplicationState) -> crate::support::BoxFuture<'static, Result<Vec<String>>>
            + Send
            + Sync
            + 'static,
    {
        self.extension_mut::<TrustedHostsRegistry>()?.0 = Some(TrustedHostsSource::Dynamic {
            callback: Arc::new(callback),
            subdomains,
        });
        Ok(self)
    }

    /// 在 Provider boot 阶段贡献共享 HTTP layer。
    pub fn http_layer<L>(&mut self, layer: L) -> Result<&mut Self>
    where
        L: Layer<AxumRoute> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future:
            Future<Output = Result<Response, Infallible>> + Send + 'static,
    {
        self.http_layer_factory(move || Ok(layer.clone()))
    }

    // 延迟到最终 Router 装配读取领域配置；沿同一层注册表传播错误。
    pub(crate) fn http_layer_factory<L, F>(&mut self, factory: F) -> Result<&mut Self>
    where
        F: Fn() -> Result<L> + Send + Sync + 'static,
        L: Layer<AxumRoute> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future:
            Future<Output = Result<Response, Infallible>> + Send + 'static,
    {
        self.extension_mut::<RouterLayerRegistry>()?
            .builders
            .push(Box::new(move |router| Ok(router.layer(factory()?))));
        Ok(self)
    }
}

/// 根据所有已注册的路由构建最终 [`AxumRouter`]，并套用默认 middleware stack。
///
/// 通过 `BootContext::routes` 注册的路由若在组装时 panic（例如 Axum 检测到重复路由），
/// 此函数会将其转换为启动错误 `http_route_registration_failed`。直接调用底层 Axum 构建 API
/// 不在此错误转换范围内。
#[allow(
    clippy::too_many_lines,
    reason = "路由装配在同一入口保持 Provider、协议及中间件顺序"
)]
pub fn build_router(app: &Application) -> Result<AxumRouter> {
    validate_trusted_proxy_ranges(app.config().server.trusted_proxy_ranges.as_deref())?;
    crate::http::proxy::validate_headers(app.config().server.trusted_proxy_headers)?;
    let mut route_urls = None;
    let mut stateful = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut middleware_registry = app
            .extension::<RouteRegistry>()
            .map(|registry| registry.middleware.clone())
            .unwrap_or_default();
        let signed_except = app
            .extension::<RouteRegistry>()
            .map(|registry| registry.signed_except.clone())
            .unwrap_or_default();
        middleware_registry.install_framework_defaults(signed_except)?;
        let cookie = app
            .state()
            .container()
            .get_ready::<crate::http::cookie::CookieLayerPlan>();
        #[cfg(feature = "session")]
        let session = app
            .extension::<Option<crate::session::SessionRuntime>>()
            .and_then(Option::as_ref)
            .map(crate::session::SessionRuntime::layer)
            .transpose()?
            .flatten();
        #[cfg(feature = "session")]
        let forgery = cookie.as_ref().map(|cookie| {
            let policy = app
                .extension::<RouteRegistry>()
                .map_or_else(crate::http::PreventRequestForgery::new, |registry| {
                    registry.forgery.clone()
                });
            crate::http::forgery::ForgeryRuntime {
                policy,
                cookie: Arc::clone(cookie),
                xsrf: session.as_ref().map(|layer| layer.settings.config.clone()),
                repository: app.config_repository(),
                testing_boot: app
                    .extension::<crate::foundation::provider::TestingBoot>()
                    .is_some()
                    && app.config().app.env == crate::config::Environment::Testing,
            }
        });
        middleware_registry.install_web_defaults(
            cookie,
            #[cfg(feature = "session")]
            session,
            #[cfg(feature = "session")]
            forgery,
        )?;
        middleware_registry.validate()?;
        let mut routes = Router::with_middleware_registry(Arc::new(middleware_registry));
        if let Some(registry) = app.extension::<RouteRegistry>() {
            for builder in &registry.builders {
                routes = builder(routes);
            }
        }
        let mut parts = routes.into_parts();
        if !parts.name_errors.is_empty() {
            return Err(Error::custom(
                "http_route_name_without_route",
                "a route name was assigned before a route was registered",
            ));
        }
        if !parts.middleware_errors.is_empty() {
            return Err(parts.middleware_errors.remove(0));
        }
        if !parts.route_errors.is_empty() {
            return Err(parts.route_errors.remove(0));
        }
        route_urls = Some(route_url_generator(app, &mut parts)?);
        let app_router = assemble(parts).with_state(app.state().clone());
        crate::http::site::mount(app_router, &app.config().server, app.state().clone())
    }))
    .map_err(|_| {
        Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "http_route_registration_failed",
            "HTTP route registration failed; check route paths and duplicate registrations",
        )
    })??;
    if let Some(layers) = app.extension::<RouterLayerRegistry>() {
        for build_layer in &layers.builders {
            stateful = build_layer(stateful)?;
        }
    }
    let calendar = app
        .extension::<crate::support::CalendarContext>()
        .cloned()
        .map_or_else(
            || {
                app.config().app.resolved_timezone().map(|zone| {
                    crate::support::CalendarContext::new(
                        zone,
                        Arc::new(crate::support::SystemClock),
                    )
                })
            },
            Ok,
        )?;
    let stateful = stateful.layer(axum::middleware::from_fn_with_state(
        calendar,
        expose_calendar_context,
    ));
    let exceptions = Arc::new(
        app.extension::<ExceptionRegistry>()
            .map(|registry| registry.0.clone())
            .unwrap_or_default(),
    );
    let app_router = middleware::apply(
        stateful,
        &app.config().server,
        &app.config().api,
        Arc::clone(&exceptions),
        app.config().app.debug,
    )?;
    let app_router = app_router.layer(axum::middleware::from_fn_with_state(
        route_urls.ok_or_else(|| {
            Error::custom(
                "http_route_url_unavailable",
                "route URL table was not initialized",
            )
        })?,
        expose_route_urls,
    ));
    let source = app
        .extension::<TrustedHostsRegistry>()
        .and_then(|registry| registry.0.clone())
        .or_else(|| {
            app.config()
                .server
                .trusted_hosts
                .clone()
                .map(TrustedHostsSource::Static)
        });
    let source = if app.is_local() || app.running_unit_tests() {
        None
    } else {
        source
    };
    Ok(app_router.layer(axum::middleware::from_fn_with_state(
        ExceptionRuntime {
            state: app.state().clone(),
            exceptions,
            debug: app.config().app.debug,
            trusted_hosts: TrustedHostsRuntime {
                app: app.state().clone(),
                source,
            },
        },
        expose_app_state,
    )))
}

fn route_url_generator(
    app: &Application,
    parts: &mut crate::routing::router::RouteParts,
) -> Result<UrlGenerator> {
    let mut generator = if parts.names.is_empty() && parts.actions.is_empty() {
        UrlGenerator::empty()
    } else if parts.actions.is_empty() {
        UrlGenerator::new(&app.config().app.url, std::mem::take(&mut parts.names))?
    } else {
        UrlGenerator::new_with_actions(
            &app.config().app.url,
            std::mem::take(&mut parts.names),
            std::mem::take(&mut parts.actions),
        )?
    };
    generator.set_get_paths(std::mem::take(&mut parts.get_paths));
    if let Some(namespace) = app
        .extension::<RouteRegistry>()
        .and_then(|registry| registry.root_controller_namespace.as_deref())
    {
        generator.set_root_controller_namespace(namespace)?;
    }
    let repository = app.config_repository();
    generator.set_key_resolver(move || {
        let values = repository.get_many(&["app.key", "app.previous_keys"])?;
        let current = match values.first().map(|(_, value)| value) {
            Some(serde_json::Value::String(key)) => key.as_bytes().to_vec(),
            Some(serde_json::Value::Null | serde_json::Value::Bool(false)) | None => Vec::new(),
            Some(serde_json::Value::Number(number)) => number.to_string().into_bytes(),
            Some(serde_json::Value::Bool(true)) => b"1".to_vec(),
            _ => {
                return Err(Error::custom(
                    "url_signing_key_invalid",
                    "app.key must be a scalar value",
                ));
            }
        };
        let previous = match values.get(1).map(|(_, value)| value) {
            Some(serde_json::Value::Array(values)) => values
                .iter()
                .map(|value| match value {
                    serde_json::Value::String(key) => Ok(key.as_bytes().to_vec()),
                    serde_json::Value::Number(number) => Ok(number.to_string().into_bytes()),
                    _ => Err(Error::custom(
                        "url_signing_keys_invalid",
                        "app.previous_keys must contain strings or numbers",
                    )),
                })
                .collect::<Result<Vec<_>>>()?,
            Some(serde_json::Value::String(keys)) => keys
                .split(',')
                .filter(|key| !key.is_empty() && *key != "0")
                .map(|key| key.as_bytes().to_vec())
                .collect(),
            Some(serde_json::Value::Null) | None => Vec::new(),
            _ => {
                return Err(Error::custom(
                    "url_signing_keys_invalid",
                    "app.previous_keys must be an array",
                ));
            }
        };
        Ok(crate::routing::url_generator::UrlSigningKeys::new(
            current, previous,
        ))
    });
    Ok(generator)
}

async fn expose_route_urls(
    State(urls): State<UrlGenerator>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(urls);
    next.run(request).await
}

#[derive(Clone)]
struct ExceptionRuntime {
    state: ApplicationState,
    exceptions: Arc<ExceptionHandler>,
    debug: bool,
    trusted_hosts: TrustedHostsRuntime,
}

async fn expose_app_state(
    State(runtime): State<ExceptionRuntime>,
    mut request: Request,
    next: Next,
) -> Response {
    #[cfg(feature = "session")]
    {
        let extensions = request.extensions_mut();
        extensions.remove::<crate::http::forgery::CsrfVerified>();
        extensions.remove::<crate::http::cookie::CookieProtection>();
        crate::http::cookie::middleware::clear_prepared(extensions);
    }
    let state = runtime.state.clone();
    let trusted_hosts = match runtime.trusted_hosts.snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => return render_early_error(&runtime, &request, error),
    };
    // 物理 Host 必须先通过 authority 语法与重复头检查；模式匹配使用可信代理后的有效 Host。
    let physical_host = if runtime.trusted_hosts.source.is_some() {
        match authority(request.headers(), request.uri()) {
            Ok(host) => Some(host),
            Err(error) => return render_early_error(&runtime, &request, error),
        }
    } else {
        None
    };
    if crate::http::proxy::platform_host(request.headers(), request.uri()) {
        request.headers_mut().remove("x-forwarded-host");
    }
    let proxy = crate::http::proxy::ProxySnapshot::capture(
        request.headers(),
        request.uri(),
        request.extensions(),
        Some(&state.config().server),
    );
    if let Some(physical_host) = physical_host {
        let effective_host = match proxy
            .context()
            .and_then(crate::http::proxy::ProxyContext::host)
        {
            Ok(Some((host, _))) => host,
            Ok(None) => physical_host,
            Err(error) => return render_early_error(&runtime, &request, error),
        };
        if let Err(error) = trusted_hosts.check(&effective_host) {
            return render_early_error(&runtime, &request, error);
        }
    }
    request.extensions_mut().insert(trusted_hosts);
    request.extensions_mut().insert(proxy);
    let scope = match state.container().scope() {
        Ok(scope) => Arc::new(scope),
        Err(error) => return render_early_error(&runtime, &request, error),
    };
    let mut close_on_cancel = ScopeCloseGuard(Some(Arc::clone(&scope)));
    let entry = Arc::new(install_http_entry(request.extensions_mut(), scope));
    request.extensions_mut().insert(state);
    let page = crate::database::query::PageContext::from_uri(request.uri());
    let (mut parts, body) = request.into_parts();
    let info = match crate::http::RequestInfo::from_request_parts(&mut parts, &()).await {
        Ok(info) => info,
        Err(error) => {
            let request = Request::from_parts(parts, body);
            return render_early_error(&runtime, &request, error);
        }
    };
    let page = page.with_path_resolver(move || info.url());
    let request = Request::from_parts(parts, body);
    let response = HTTP_ENTRY_SCOPE
        .scope(
            Arc::clone(&entry),
            entry.scope.run(crate::database::query::with_page_context(
                page,
                next.run(request),
            )),
        )
        .await
        .map(move |body| {
            Body::new(ScopeBody {
                inner: body,
                entry: Some(entry),
            })
        });
    // 响应 body 已接管关闭责任；handler future 取消时 guard 仍会同步封闭 scope。
    close_on_cancel.0.take();
    response
}

fn install_http_entry(extensions: &mut http::Extensions, scope: Arc<Scope>) -> HttpEntry {
    let route = extensions
        .get::<RouteSlot>()
        .filter(|route| route.claim_http_entry())
        .cloned()
        .unwrap_or_else(|| {
            let route = RouteSlot::default();
            let _ = route.claim_http_entry();
            route
        });
    let attributes = RequestAttributes::default();
    extensions.insert(route.clone());
    extensions.insert(attributes.clone());
    extensions.insert(Arc::clone(&scope));
    HttpEntry {
        scope,
        attributes,
        route,
    }
}

fn render_early_error(runtime: &ExceptionRuntime, request: &Request, error: Error) -> Response {
    let view = ExceptionRequest::new(
        request.method().clone(),
        request.uri().path(),
        crate::http::request::expects_json_headers(request.headers()),
        "-",
        runtime.debug,
    );
    #[cfg(feature = "view")]
    let view = view.with_views(runtime.state.get::<crate::view::Views>());
    runtime.exceptions.render_for_request(error, &view)
}

struct ScopeCloseGuard(Option<Arc<Scope>>);

impl Drop for ScopeCloseGuard {
    fn drop(&mut self) {
        if let Some(scope) = self.0.take() {
            scope.close();
        }
    }
}

struct ScopeBody {
    inner: Body,
    entry: Option<Arc<HttpEntry>>,
}

impl ScopeBody {
    fn close(&mut self) {
        if let Some(entry) = self.entry.take() {
            entry.scope.close();
        }
    }
}

impl HttpBody for ScopeBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let result = if let Some(entry) = &this.entry {
            let mut scoped = std::pin::pin!(HTTP_ENTRY_SCOPE.scope(
                Arc::clone(entry),
                entry.scope.run(std::future::poll_fn(|cx| {
                    Pin::new(&mut this.inner).poll_frame(cx)
                })),
            ));
            scoped.as_mut().poll(context)
        } else {
            Pin::new(&mut this.inner).poll_frame(context)
        };
        if matches!(&result, Poll::Ready(None | Some(Err(_)))) {
            this.close();
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for ScopeBody {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HttpServiceProvider, ScopeBody, current_request_attribute, current_route_parameter,
        http_entry_scope, install_http_entry,
    };
    use crate::container::{ContainerBuilder, ParameterValue};
    use crate::foundation::ProviderBuilder;
    use crate::{Error, Result};
    use axum::body::{Body, Bytes, to_bytes};
    use figment::{Figment, providers::Serialized};
    use std::sync::Arc;

    #[tokio::test]
    async fn invalid_shutdown_timeout_fails_during_provider_boot() {
        let mut providers = ProviderBuilder::new();
        providers.provider(HttpServiceProvider);
        let sources = Figment::from(Serialized::defaults(serde_json::json!({
            "server": {"shutdown_timeout_secs": 0}
        })));
        let result = providers.start(sources).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn response_body_poll_restores_only_its_physical_request_context() -> Result<()> {
        let container = ContainerBuilder::new().build();
        let scope = Arc::new(container.scope()?);
        let resolver = scope.factory_resolver()?;
        let root = container.factory_resolver()?;
        let manual_scope = Arc::new(container.scope()?);
        let manual = manual_scope.factory_resolver()?;
        let mut extensions = http::Extensions::new();
        let entry = install_http_entry(&mut extensions, Arc::clone(&scope));
        let _ = entry.attributes.insert("item", ParameterValue::new(7_u8))?;
        entry.route.publish(
            crate::routing::Route::new(
                None,
                "stream/{item}",
                None,
                vec!["GET".to_owned()],
                Vec::new(),
            )
            .with_bound_parameter("item", ParameterValue::new(9_u8)),
        )?;
        assert!(current_request_attribute(&resolver, "item")?.is_none());

        let inner = Body::from_stream(futures_util::stream::once(async move {
            assert!(http_entry_scope().is_some());
            let attribute = current_request_attribute(&resolver, "item")?.ok_or_else(|| {
                Error::custom("test_stream_attribute_missing", "响应体未恢复请求属性")
            })?;
            let route = current_route_parameter(&resolver, "item")?.ok_or_else(|| {
                Error::custom("test_stream_route_missing", "响应体未恢复绑定的路由参数")
            })?;
            assert!(current_request_attribute(&root, "item")?.is_none());
            manual_scope
                .run(async {
                    assert!(current_request_attribute(&manual, "item")?.is_none());
                    assert!(current_route_parameter(&manual, "item")?.is_none());
                    Ok::<(), Error>(())
                })
                .await?;
            Ok::<Bytes, Error>(Bytes::copy_from_slice(&[
                *attribute.downcast::<u8>()?,
                *route.downcast::<u8>()?,
            ]))
        }));
        let body = Body::new(ScopeBody {
            inner,
            entry: Some(Arc::new(entry)),
        });
        assert_eq!(
            to_bytes(body, 2).await.map_err(Error::internal)?.as_ref(),
            [7, 9]
        );
        assert!(http_entry_scope().is_none());
        assert!(matches!(scope.factory_resolver(), Err(error) if error.code() == "scope_closed"));
        Ok(())
    }
}

async fn expose_calendar_context(
    State(context): State<crate::support::CalendarContext>,
    mut request: Request,
    next: Next,
) -> Response {
    if request
        .extensions()
        .get::<crate::support::CalendarContext>()
        .is_none()
    {
        request.extensions_mut().insert(context);
    }
    next.run(request).await
}

#[cfg(test)]
mod parameter_alias_router_tests {
    use super::*;
    use crate::config::NoConfig;
    use crate::foundation::ApplicationDefinition;
    use crate::routing::RouteGroup;
    use crate::support::{BoxFuture, DefaultProviders, ServiceProvider};
    use crate::testing::TestApp;
    use figment::{Figment, providers::Serialized};
    use std::sync::Mutex;

    #[derive(Clone)]
    struct Trace(Arc<Mutex<Vec<String>>>);
    async fn record(
        State((label, trace)): State<(String, Trace)>,
        request: Request,
        next: Next,
    ) -> Response {
        trace
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(label);
        next.run(request).await
    }
    struct ParameterProvider;
    impl ServiceProvider for ParameterProvider {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let trace = app.resolve::<Trace>().await?;
                for name in ["auth", "throttle"] {
                    let trace = (*trace).clone();
                    app.middleware_alias_with_parameters(name, move |arguments| {
                        if arguments.len() != 1 || arguments[0].is_empty() {
                            return Err(Error::custom(
                                "middleware_parameters_invalid",
                                "要求一个非空参数",
                            ));
                        }
                        Ok(axum::middleware::from_fn_with_state(
                            (format!("{name}:{}", arguments[0]), trace.clone()),
                            record,
                        ))
                    })?;
                }
                app.middleware_priority(["auth", "throttle"])?;
                app.middleware_group("parameterized", ["throttle:slow", "auth:staff"])?;
                app.routes(|routes| {
                    routes.group(
                        RouteGroup::new().middleware_alias("parameterized"),
                        |routes| routes.get("/parameterized", || async { "executed" }),
                    )
                })?;
                Ok(())
            })
        }
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(Trace(Arc::new(Mutex::new(Vec::new())))))
                    .await
            })
        }
    }
    struct InvalidParameterProvider;
    impl ServiceProvider for InvalidParameterProvider {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.middleware_alias_with_parameters("auth", |arguments| {
                    if arguments.len() != 1 || arguments[0].is_empty() {
                        return Err(Error::custom(
                            "middleware_parameters_invalid",
                            "要求一个非空参数",
                        ));
                    }
                    Ok(axum::middleware::from_fn_with_state(
                        (
                            arguments[0].clone(),
                            Trace(Arc::new(Mutex::new(Vec::new()))),
                        ),
                        record,
                    ))
                })?;
                app.routes(|routes| {
                    routes.group(RouteGroup::new().middleware_alias("auth:"), |routes| {
                        routes.get("/invalid", || async { "must not run" })
                    })
                })?;
                Ok(())
            })
        }
    }
    struct InvalidParameterApp;
    impl ApplicationDefinition for InvalidParameterApp {
        type Config = NoConfig;
        fn default_providers() -> DefaultProviders {
            DefaultProviders::empty()
        }
        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![
                Box::new(HttpServiceProvider),
                Box::new(InvalidParameterProvider),
            ]
        }
    }
    #[tokio::test]
    async fn invalid_parameters_prevent_real_application_router_startup() {
        let result = TestApp::<InvalidParameterApp>::builder(Figment::from(Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        )))
        .build()
        .await;
        assert!(result.is_err());
    }
    struct ParameterApp;
    impl ApplicationDefinition for ParameterApp {
        type Config = NoConfig;
        fn default_providers() -> DefaultProviders {
            DefaultProviders::empty()
        }
        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(HttpServiceProvider), Box::new(ParameterProvider)]
        }
    }
    #[tokio::test]
    async fn real_router_passes_parameters_and_orders_aliases_by_base_name() -> Result<()> {
        let fixture = TestApp::<ParameterApp>::builder(Figment::from(Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        )))
        .build()
        .await?;
        let response = fixture
            .raw_response(
                Request::get("/parameterized")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let trace = fixture.app()?.state().resolve::<Trace>().await?;
        assert_eq!(
            *trace
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec!["auth:staff", "throttle:slow"]
        );
        Ok(())
    }
}

/// 在HTTP Provider原有boot位置登记健康探针和显式指标路由。
pub(crate) fn register_application_routes(
    app: &mut BootContext<'_>,
    metrics_endpoint_enabled: bool,
) -> Result<()> {
    app.routes(health::routes)?;
    if metrics_endpoint_enabled {
        #[cfg(feature = "metrics")]
        {
            let _ = crate::http::metrics::initialize()?;
            app.routes(crate::http::metrics::routes)?;
        }
        #[cfg(not(feature = "metrics"))]
        return Err(crate::config::ConfigError::Invalid(
            "server.metrics_endpoint_enabled requires the `metrics` feature".into(),
        )
        .into());
    }
    Ok(())
}

#[cfg(all(test, feature = "session", feature = "encryption"))]
mod http_scope_origin_tests {
    use super::*;
    use crate::container::Scope;
    use crate::http::{CookieServiceProvider, HttpServiceProvider};
    use crate::session::{
        MemorySessionStore, SessionId, SessionRecord, SessionServiceProvider, SessionStore,
        SessionWriteMode, SessionWriteOutcome,
    };
    use crate::support::{BoxFuture, ServiceProvider};
    use figment::{Figment, providers::Serialized};
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tower::ServiceExt;

    #[derive(Default)]
    struct Observation {
        source: Mutex<Option<Arc<Scope>>>,
        loads: AtomicUsize,
        writes: AtomicUsize,
        handlers: AtomicUsize,
    }

    struct Store {
        memory: MemorySessionStore,
        observed: Arc<Observation>,
    }

    impl SessionStore for Store {
        fn load<'a>(
            &'a self,
            id: &'a SessionId,
            limit: usize,
            expiry: time::OffsetDateTime,
        ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
            self.observed.loads.fetch_add(1, Ordering::SeqCst);
            self.memory.load(id, limit, expiry)
        }

        fn write<'a>(
            &'a self,
            record: &'a SessionRecord,
            mode: SessionWriteMode,
            limit: usize,
        ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
            self.observed.writes.fetch_add(1, Ordering::SeqCst);
            self.memory.write(record, mode, limit)
        }

        fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
            self.memory.delete(id)
        }
    }

    async fn rebuild_or_replace_scope(
        State(observed): State<Arc<Observation>>,
        request: Request,
        next: Next,
    ) -> Result<Response> {
        let (mut parts, body) = request.into_parts();
        let failure = || Error::custom("test_scope_unavailable", "测试作用域不可用");
        if parts.uri.path() == "/scope-source" {
            let scope = parts.extensions.get::<Arc<Scope>>().ok_or_else(failure)?;
            *observed.source.lock().map_err(|_| failure())? = Some(Arc::clone(scope));
        }
        if parts.headers.contains_key("x-test-replace-scope") {
            let foreign = observed
                .source
                .lock()
                .map_err(|_| failure())?
                .clone()
                .ok_or_else(failure)?;
            parts.extensions.insert(Arc::clone(&foreign));
            // 即使应用同时替换公开容器上下文，HTTP 入口仍拥有另一作用域。
            return Ok(foreign
                .run(next.run(Request::from_parts(parts, body)))
                .await);
        }
        Ok(next.run(Request::from_parts(parts, body)).await)
    }

    struct Routes(Arc<Observation>);

    impl ServiceProvider for Routes {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.http_layer(axum::middleware::from_fn_with_state(
                    Arc::clone(&self.0),
                    rebuild_or_replace_scope,
                ))?;
                app.exceptions(|handler| {
                    handler.api_envelope_when(|request, _| request.path() == "/scope-protected");
                })?;
                let observed = Arc::clone(&self.0);
                app.routes(move |routes| {
                    routes
                        .get("/scope-source", || async { "live source" })
                        .group(
                            crate::routing::RouteGroup::new().middleware_group("web"),
                            |routes| {
                                let observed = Arc::clone(&observed);
                                routes.get("/scope-protected", move || {
                                    let observed = Arc::clone(&observed);
                                    async move {
                                        observed.handlers.fetch_add(1, Ordering::SeqCst);
                                        "protected"
                                    }
                                })
                            },
                        )
                })?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn global_layer_cannot_lease_another_live_http_scope() -> Result<()> {
        let observed = Arc::new(Observation::default());
        let mut providers = crate::foundation::ProviderBuilder::new();
        providers
            .provider(HttpServiceProvider)
            .provider(crate::encryption::EncryptionServiceProvider)
            .provider(CookieServiceProvider::contextual())
            .provider(
                SessionServiceProvider::with_store(Store {
                    memory: MemorySessionStore::default(),
                    observed: Arc::clone(&observed),
                })
                .with_cookie_config(crate::config::SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some("test_session".into()),
                    secure: Some(false),
                    ..Default::default()
                }),
            )
            .provider(Routes(Arc::clone(&observed)));
        let mut config = crate::config::FrameworkConfig::default();
        config.app.env = crate::config::Environment::Development;
        let sources = Figment::from(Serialized::defaults(config)).merge((
            "security",
            serde_json::json!({
                "encryption": {
                    "active_key_id": "test",
                    "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}
                }
            }),
        ));
        let app = Application::from_provider_runtime(providers.start(sources).await?)?;
        let router = build_router(&app)?;
        let source = router
            .clone()
            .oneshot(
                Request::get("/scope-source")
                    .header(http::header::HOST, "localhost")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(source.status(), StatusCode::OK);

        let foreign = router
            .clone()
            .oneshot(
                Request::get("/scope-protected")
                    .header(http::header::HOST, "localhost")
                    .header(http::header::ACCEPT, "application/json")
                    .header("x-test-replace-scope", "1")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(foreign.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            foreign.headers().get(http::header::CACHE_CONTROL),
            Some(&http::HeaderValue::from_static("no-store"))
        );
        assert!(!foreign.headers().contains_key(http::header::SET_COOKIE));
        let body = axum::body::to_bytes(foreign.into_body(), 4 * 1024)
            .await
            .map_err(Error::internal)?;
        let payload: serde_json::Value = serde_json::from_slice(&body).map_err(Error::internal)?;
        assert_eq!(payload["code"], "cookie_scope_missing");
        assert_eq!(observed.loads.load(Ordering::SeqCst), 0);
        assert_eq!(observed.writes.load(Ordering::SeqCst), 0);
        assert_eq!(observed.handlers.load(Ordering::SeqCst), 0);
        // A 的响应 body 一直存活到 B 完成，排除因 A 已关闭而恰好拒绝的情况。
        drop(source);

        let rebuilt = router
            .oneshot(
                Request::get("/scope-protected")
                    .header(http::header::HOST, "localhost")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(rebuilt.status(), StatusCode::OK);
        assert!(rebuilt.headers().contains_key(http::header::SET_COOKIE));
        assert_eq!(observed.writes.load(Ordering::SeqCst), 1);
        assert_eq!(observed.handlers.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
