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

use crate::core::{ApiResponse, App, AppState, BootContext, Error, Result};
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::Route;
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tower::{Layer, Service};

use crate::http::client_ip::validate_trusted_proxy_ranges;
use crate::http::health;
use crate::http::middleware;
pub use crate::http::routes::Routes;
use crate::http::routes::{MiddlewareLayerBuilder, MiddlewareRegistry};
use crate::http::serve::ServeCommand;

/// 注册根宿主的框架信息 API `GET /api`。
///
/// 该路由面向仓库集成宿主；网站根路径由 HTTP 静态站点配置独立管理。
pub fn framework_routes(routes: Routes) -> Routes {
    routes.get("/api", framework_info)
}

async fn framework_info(
    State(state): State<AppState>,
) -> ApiResponse<std::collections::BTreeMap<&'static str, String>> {
    ApiResponse::success(std::collections::BTreeMap::from([
        ("name", state.config().app.name.clone()),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ]))
}

type RouteBuilder = Box<dyn Fn(Routes) -> Routes + Send + Sync>;
type RouterLayerBuilder = Box<dyn Fn(Router) -> Router + Send + Sync>;

#[derive(Default)]
struct RouterLayerRegistry {
    builders: Vec<RouterLayerBuilder>,
}

/// 注册阶段收集的路由构建器；构建 router 时按顺序应用。
#[derive(Default)]
pub struct RouteRegistry {
    builders: Vec<RouteBuilder>,
    middleware: MiddlewareRegistry,
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
        self.extension_mut::<RouterLayerRegistry>()?
            .builders
            .push(Box::new(move |router| router.layer(layer.clone())));
        Ok(self)
    }
}

/// 声明 HTTP 服务，在 boot 阶段贡献 serve、健康探针与可选指标路由。
pub struct HttpServiceProvider;

impl crate::core::ServiceProvider for HttpServiceProvider {
    fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
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
            ("trusted_proxy_ranges", strings()),
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
        app.instance(Arc::new(health::ServerReadiness::default()));
        app.instance(Arc::new(HttpPluginInstalled));
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config: crate::config::ServerConfig = app.section("server")?;
            validate_http_config(&config)?;
            validate_trusted_proxy_ranges(&config.trusted_proxy_ranges)?;
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
pub fn build_router(app: &App) -> Result<Router> {
    validate_trusted_proxy_ranges(&app.config().server.trusted_proxy_ranges)?;
    let mut route_urls = None;
    let mut stateful = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let middleware_registry = app
            .extension::<RouteRegistry>()
            .map(|registry| registry.middleware.clone())
            .unwrap_or_default();
        middleware_registry.validate()?;
        let mut routes = Routes::with_middleware_registry(Arc::new(middleware_registry));
        if let Some(registry) = app.extension::<RouteRegistry>() {
            for builder in &registry.builders {
                routes = builder(routes);
            }
        }
        let (collected_router, named_routes, route_name_errors, middleware_errors) =
            routes.into_parts();
        if !route_name_errors.is_empty() {
            return Err(Error::custom(
                "http_route_name_without_route",
                "a route name was assigned before a route was registered",
            ));
        }
        if let Some(error) = middleware_errors.into_iter().next() {
            return Err(error);
        }
        route_urls = Some(if named_routes.is_empty() {
            crate::http::RouteUrlGenerator::empty()
        } else {
            crate::http::RouteUrlGenerator::new(&app.config().app.url, named_routes)?
        });
        let app_router = collected_router
            .fallback(crate::http::site::fallback)
            .method_not_allowed_fallback(|| async {
                Error::custom_with_status(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "method not allowed",
                )
            })
            .with_state(app.state().clone());
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
            stateful = build_layer(stateful);
        }
    }
    let app_router = middleware::apply(stateful, &app.config().server, &app.config().api)?;
    let app_router = app_router.layer(axum::middleware::from_fn_with_state(
        route_urls.ok_or_else(|| {
            Error::custom(
                "http_route_url_unavailable",
                "route URL table was not initialized",
            )
        })?,
        expose_route_urls,
    ));
    Ok(app_router.layer(axum::middleware::from_fn_with_state(
        app.state().clone(),
        expose_app_state,
    )))
}

async fn expose_route_urls(
    State(urls): State<crate::http::RouteUrlGenerator>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(urls);
    next.run(request).await
}

async fn expose_app_state(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let scope = match state.container().scope() {
        Ok(scope) => Arc::new(scope),
        Err(error) => return error.into_response(),
    };
    let mut close_on_cancel = ScopeCloseGuard(Some(Arc::clone(&scope)));
    request.extensions_mut().insert(state);
    request.extensions_mut().insert(Arc::clone(&scope));
    let response = scope.run(next.run(request)).await.map(|body| {
        Body::new(ScopeBody {
            inner: body,
            scope: Some(Arc::clone(&scope)),
        })
    });
    // 响应 body 已接管关闭责任；handler future 取消时 guard 仍会同步封闭 scope。
    close_on_cancel.0.take();
    response
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
