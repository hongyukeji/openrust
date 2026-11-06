//! 框架 HTTP 路由注册与应用扩展入口。
//!
//! `ServiceProvider::boot` 可通过 `BootContext::routes` 按注册顺序追加路由，
//! 并通过 `BootContext::http_layer` 应用 Tower middleware。重复路由会在 router
//! 装配时转换为启动错误；具体业务路由由应用 Provider 提供。

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::core::exception_handler::ExceptionRegistry;
use crate::core::{App, AppState, BootContext, Error, Result};
use crate::core::{ExceptionHandler, ExceptionRequest};
use axum::Router;
use axum::body::Body;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::Route;
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tower::{Layer, Service};

use crate::http::authority::authority;
use crate::http::client_ip::validate_trusted_proxy_ranges;
use crate::http::health;
use crate::http::middleware;
pub use crate::http::routes::Routes;
use crate::http::routes::{MiddlewareLayerBuilder, MiddlewareRegistry, assemble};
use crate::http::serve::ServeCommand;
use crate::http::trusted_hosts::TrustedHostsSnapshot;

type RouteBuilder = Box<dyn Fn(Routes) -> Routes + Send + Sync>;
type RouterLayerBuilder = Box<dyn Fn(Router) -> Result<Router> + Send + Sync>;

#[derive(Default)]
struct RouterLayerRegistry {
    builders: Vec<RouterLayerBuilder>,
}

type TrustedHostsCallback =
    Arc<dyn Fn(AppState) -> crate::core::BoxFuture<'static, Result<Vec<String>>> + Send + Sync>;

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
    app: AppState,
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

/// 注册阶段收集的路由构建器；构建 router 时按顺序应用。
#[derive(Default)]
pub struct RouteRegistry {
    builders: Vec<RouteBuilder>,
    middleware: MiddlewareRegistry,
    root_controller_namespace: Option<String>,
    signed_except: Vec<String>,
    #[cfg(feature = "session")]
    forgery: crate::http::PreventRequestForgery,
}

impl RouteRegistry {
    /// 已注册的路由构建器数量。
    pub fn len(&self) -> usize {
        self.builders.len()
    }

    /// 没有注册任何路由时返回 `true`。
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
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
        F: Fn(AppState) -> crate::core::BoxFuture<'static, Result<Vec<String>>>
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

    /// 设置本应用默认 web 组的请求伪造策略；仅影响该应用最终 Router。
    #[cfg(feature = "session")]
    pub fn prevent_request_forgery(
        &mut self,
        policy: crate::http::PreventRequestForgery,
    ) -> Result<&mut Self> {
        self.extension_mut::<RouteRegistry>()?.forgery = policy;
        Ok(self)
    }

    /// 在 Provider boot 阶段贡献路由。
    pub fn routes<F>(&mut self, builder: F) -> Result<&mut Self>
    where
        F: Fn(Routes) -> Routes + Send + Sync + 'static,
    {
        self.extension_mut::<RouteRegistry>()?
            .builders
            .push(Box::new(builder));
        Ok(self)
    }

    /// 设置最终动作 URL 生成器的查询根命名空间；不会修改路由组的登记空间。
    ///
    /// 空字符串清空默认空间，控制字符以 `http_route_action_invalid` 返回错误。
    pub fn set_root_controller_namespace(
        &mut self,
        namespace: impl Into<String>,
    ) -> Result<&mut Self> {
        let namespace = namespace.into();
        crate::http::action::validate_namespace(&namespace)?;
        self.extension_mut::<RouteRegistry>()?
            .root_controller_namespace = (!namespace.is_empty()).then_some(namespace);
        Ok(self)
    }

    /// 为本应用的全部默认 `signed` middleware 设置全局忽略字段。
    ///
    /// 默认与参数化别名在 router 构建时读取同一列表；另一 App 不受影响。
    pub fn signed_url_except<I, S>(&mut self, keys: I) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let except = &mut self.extension_mut::<RouteRegistry>()?.signed_except;
        for key in keys.into_iter().map(Into::into) {
            if !except.contains(&key) {
                except.push(key);
            }
        }
        Ok(self)
    }

    /// 在 Provider boot 阶段登记 middleware alias，供应用路由按名称引用。
    pub fn middleware_alias<L>(&mut self, name: impl Into<String>, layer: L) -> Result<&mut Self>
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        let builder: MiddlewareLayerBuilder = Arc::new(move |routes| routes.layer(layer.clone()));
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .alias(name.into(), builder)?;
        Ok(self)
    }

    /// 登记同一middleware registry的参数化alias；参数按逗号拆分且保留空项。
    ///
    /// 无冒号引用传入空Vec，`name:`传入一个空字符串；参数数量与语义由工厂校验。
    /// 工厂在路由装配时执行，返回错误会阻止应用启动；不能从工厂创建另一请求scope。
    pub fn middleware_alias_with_parameters<L, F>(
        &mut self,
        name: impl Into<String>,
        factory: F,
    ) -> Result<&mut Self>
    where
        F: Fn(Vec<String>) -> Result<L> + Send + Sync + 'static,
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .parameter_alias(
                name.into(),
                Arc::new(move |parameters| {
                    let layer = factory(parameters)?;
                    let builder: MiddlewareLayerBuilder =
                        Arc::new(move |routes| routes.layer(layer.clone()));
                    Ok(builder)
                }),
            )?;
        Ok(self)
    }

    /// 在 Provider boot 阶段登记由 alias 或其他 group 组成的 middleware group。
    pub fn middleware_group<I, S>(
        &mut self,
        name: impl Into<String>,
        members: I,
    ) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let members = members.into_iter().map(Into::into).collect();
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .group(name.into(), members)?;
        Ok(self)
    }

    /// 设置路由命名 middleware 的优先级；不启用或自动挂载其中的 alias。
    ///
    /// 可先于 alias 登记；组展开后沿统一路由链排序，未列出的项保持相对顺序。
    /// 全局 `http_layer` 与原生 Axum 逃生入口不属于此路由优先级表。
    pub fn middleware_priority<I, S>(&mut self, names: I) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .priority(names.into_iter().map(Into::into).collect())?;
        Ok(self)
    }

    /// 将一个 alias 放到路由优先级表首部；不改变全局 HTTP layer。
    pub fn prepend_middleware_priority(&mut self, name: impl Into<String>) -> Result<&mut Self> {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .prepend_priority(name.into())?;
        Ok(self)
    }

    /// 将一个 alias 放到路由优先级表尾部；不改变全局 HTTP layer。
    pub fn append_middleware_priority(&mut self, name: impl Into<String>) -> Result<&mut Self> {
        self.extension_mut::<RouteRegistry>()?
            .middleware
            .append_priority(name.into())?;
        Ok(self)
    }

    /// 在 Provider boot 阶段贡献共享 HTTP layer。
    pub fn http_layer<L>(&mut self, layer: L) -> Result<&mut Self>
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
        self.http_layer_factory(move || Ok(layer.clone()))
    }

    // 延迟到最终 Router 装配读取领域配置；沿同一层注册表传播错误。
    pub(crate) fn http_layer_factory<L, F>(&mut self, factory: F) -> Result<&mut Self>
    where
        F: Fn() -> Result<L> + Send + Sync + 'static,
        L: Layer<Route> + Clone + Send + Sync + 'static,
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

/// 声明 HTTP 服务，在 boot 阶段贡献 serve、健康探针与可选指标路由。
pub struct HttpServiceProvider;

impl crate::core::ServiceProvider for HttpServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as Schema;
            let strings = || Schema::array(Schema::String);
            let schema = Schema::object([
                ("host", Schema::String),
                ("port", Schema::Integer),
                ("public_dir", Schema::String),
                ("root_target", Schema::String),
                ("request_timeout_secs", Schema::Integer),
                ("max_concurrent_requests", Schema::Integer),
                ("shutdown_timeout_secs", Schema::Integer),
                ("metrics_endpoint_enabled", Schema::Boolean),
                ("trusted_proxy_ranges", strings().nullable()),
                ("trusted_proxy_headers", Schema::Integer),
                (
                    "trusted_hosts",
                    Schema::object([
                        ("at", strings().nullable()),
                        ("subdomains", Schema::Boolean),
                    ])
                    .nullable(),
                ),
                (
                    "compression",
                    Schema::object([("enabled", Schema::Boolean)]),
                ),
                (
                    "cors",
                    Schema::object([
                        ("enabled", Schema::Boolean),
                        ("allowed_origins", strings()),
                        ("allowed_methods", strings()),
                        ("allowed_headers", strings()),
                        ("expose_headers", strings()),
                        ("allow_credentials", Schema::Boolean),
                        ("max_age_secs", Schema::Integer),
                    ]),
                ),
            ]);
            app.config_section(
                "server",
                crate::config::ServerConfig::default(),
                schema,
                validate_http_config,
            )?;
            app.instance(Arc::new(health::ServerReadiness::default()))
                .await?;
            app.instance(Arc::new(HttpPluginInstalled)).await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config: crate::config::ServerConfig = app.section("server")?;
            validate_http_config(&config)?;
            validate_trusted_proxy_ranges(config.trusted_proxy_ranges.as_deref())?;
            crate::http::proxy::validate_headers(config.trusted_proxy_headers)?;
            let _ = app.resolve::<health::ServerReadiness>().await?;
            app.command(ServeCommand);
            app.routes(health::routes)?;
            if config.metrics_endpoint_enabled {
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
        })
    }
}

fn validate_http_config(config: &crate::config::ServerConfig) -> Result<()> {
    if !(1..=3600).contains(&config.shutdown_timeout_secs) {
        return Err(crate::config::ConfigError::Invalid(
            "server.shutdown_timeout_secs must be 1..=3600 seconds".into(),
        )
        .into());
    }
    if !(1..=65_536).contains(&config.max_concurrent_requests) {
        return Err(crate::config::ConfigError::Invalid(
            "server.max_concurrent_requests must be 1..=65536".into(),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    #[tokio::test]
    async fn http_provider_contributes_health_and_command() -> Result<()> {
        let mut providers = crate::core::ProviderBuilder::new();
        providers.provider(HttpServiceProvider);
        let runtime = providers.start(Figment::new()).await?;
        assert!(
            runtime
                .container()
                .resolve::<health::ServerReadiness>()
                .await
                .is_ok()
        );
        assert_eq!(
            runtime.extension::<RouteRegistry>().map(RouteRegistry::len),
            Some(1)
        );
        assert!(runtime.commands().any(|command| command.name() == "serve"));
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn http_provider_rejects_invalid_server_config() {
        let mut providers = crate::core::ProviderBuilder::new();
        providers.provider(HttpServiceProvider);
        let sources = Figment::from(Serialized::defaults(
            json!({"server":{"max_concurrent_requests":0}}),
        ));
        assert!(providers.start(sources).await.is_err());
    }
}

/// HTTP Provider 已安装的内部能力标记。
#[derive(Clone, Copy)]
pub(crate) struct HttpPluginInstalled;

/// 根据所有已注册的路由构建最终 [`Router`]，并套用默认 middleware stack。
///
/// 通过 `BootContext::routes` 注册的路由若在组装时 panic（例如 Axum 检测到重复路由），
/// 此函数会将其转换为启动错误 `http_route_registration_failed`。直接调用底层 Axum 构建 API
/// 不在此错误转换范围内。
#[allow(
    clippy::too_many_lines,
    reason = "路由装配在同一入口保持 Provider、协议及中间件顺序"
)]
pub fn build_router(app: &App) -> Result<Router> {
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
            .extension::<crate::http::cookie::CookieRuntime>()
            .and_then(|runtime| runtime.0.clone());
        #[cfg(feature = "session")]
        let session = app
            .extension::<Option<crate::session::SessionRuntime>>()
            .and_then(Option::as_ref)
            .map(crate::session::SessionRuntime::layer)
            .transpose()?
            .flatten();
        #[cfg(feature = "session")]
        let forgery = cookie
            .as_ref()
            .zip(session.as_ref())
            .map(|(cookie, session)| crate::http::forgery::ForgeryRuntime {
                policy: app
                    .extension::<RouteRegistry>()
                    .map_or_else(crate::http::PreventRequestForgery::new, |registry| {
                        registry.forgery.clone()
                    }),
                cookie: cookie.clone(),
                session: session.settings.config.clone(),
                session_cookie_name: session.settings.cookie_name.clone(),
                testing_boot: app
                    .extension::<crate::core::provider::TestingBoot>()
                    .is_some()
                    && app.config().app.env == crate::config::Environment::Testing,
            });
        middleware_registry.install_web_defaults(
            cookie,
            #[cfg(feature = "session")]
            session,
            #[cfg(feature = "session")]
            forgery,
        )?;
        middleware_registry.validate()?;
        let mut routes = Routes::with_middleware_registry(Arc::new(middleware_registry));
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
    app: &App,
    parts: &mut crate::http::routes::RouteParts,
) -> Result<crate::http::RouteUrlGenerator> {
    let mut generator = if parts.names.is_empty() && parts.actions.is_empty() {
        crate::http::RouteUrlGenerator::empty()
    } else if parts.actions.is_empty() {
        crate::http::RouteUrlGenerator::new(
            &app.config().app.url,
            std::mem::take(&mut parts.names),
        )?
    } else {
        crate::http::RouteUrlGenerator::new_with_actions(
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
        Ok(crate::http::url::UrlSigningKeys::new(current, previous))
    });
    Ok(generator)
}

async fn expose_route_urls(
    State(urls): State<crate::http::RouteUrlGenerator>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(urls);
    next.run(request).await
}

#[derive(Clone)]
struct ExceptionRuntime {
    state: AppState,
    exceptions: Arc<ExceptionHandler>,
    debug: bool,
    trusted_hosts: TrustedHostsRuntime,
}

async fn expose_app_state(
    State(runtime): State<ExceptionRuntime>,
    mut request: Request,
    next: Next,
) -> Response {
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
    request.extensions_mut().insert(state);
    if request
        .extensions()
        .get::<crate::http::route::RouteSlot>()
        .is_none()
    {
        request
            .extensions_mut()
            .insert(crate::http::route::RouteSlot::default());
    }
    request.extensions_mut().insert(Arc::clone(&scope));
    let page = crate::db::query::PageContext::from_uri(request.uri());
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
    let response = scope
        .run(crate::db::query::with_page_context(page, next.run(request)))
        .await
        .map(|body| {
            Body::new(ScopeBody {
                inner: body,
                scope: Some(Arc::clone(&scope)),
            })
        });
    // 响应 body 已接管关闭责任；handler future 取消时 guard 仍会同步封闭 scope。
    close_on_cancel.0.take();
    response
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

struct ScopeCloseGuard(Option<Arc<crate::core::container::Scope>>);

impl Drop for ScopeCloseGuard {
    fn drop(&mut self) {
        if let Some(scope) = self.0.take() {
            scope.close();
        }
    }
}

struct ScopeBody {
    inner: Body,
    scope: Option<Arc<crate::core::container::Scope>>,
}

impl ScopeBody {
    fn close(&mut self) {
        if let Some(scope) = self.scope.take() {
            scope.close();
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
        let result = if let Some(scope) = &this.scope {
            let mut scoped = std::pin::pin!(scope.run(std::future::poll_fn(|cx| {
                Pin::new(&mut this.inner).poll_frame(cx)
            })));
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
    use super::HttpServiceProvider;
    use crate::core::ProviderBuilder;
    use figment::{Figment, providers::Serialized};

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
    use crate::{
        config::NoConfig,
        core::{Application, BoxFuture, DefaultProviders, ServiceProvider},
        http::RouteGroup,
        testing::TestApp,
    };
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
            app: &'a mut crate::core::RegisterContext<'_>,
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
    impl Application for InvalidParameterApp {
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
    impl Application for ParameterApp {
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
