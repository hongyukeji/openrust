//! [`HttpPlugin`] 与路由注册扩展。

use std::convert::Infallible;
use std::future::Future;

use crate::core::{App, AppBuilder, AppState, Error, Plugin, Result};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::Route;
use tower::{Layer, Service};

use crate::http::client_ip::validate_trusted_proxy_ranges;
use crate::http::health;
use crate::http::middleware;
use crate::http::routes::Routes;
use crate::http::serve::ServeCommand;

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

/// 向 [`AppBuilder`] 添加路由和 Tower layer。
pub trait AppBuilderHttpExt {
    /// 提供路由。请在 [`Application::build`](crate::core::Application::build) 中调用，
    /// 或在插件的 `build` 阶段调用；构建器按注册顺序运行。
    fn routes<F>(&mut self, builder: F) -> &mut Self
    where
        F: Fn(Routes) -> Routes + Send + Sync + 'static;

    /// 路由注册完成后，为所有路由添加 Tower layer。
    ///
    /// 能力插件可用此方法安装请求上下文 middleware，使其运行在路由级 layer 之外。
    /// 默认 HTTP 响应/错误 middleware 仍运行在此 layer 之外。
    fn http_layer<L>(&mut self, layer: L) -> &mut Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future:
            Future<Output = Result<Response, Infallible>> + Send + 'static;
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

    fn http_layer<L>(&mut self, layer: L) -> &mut Self
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
        self.extension_mut::<RouterLayerRegistry>()
            .builders
            .push(Box::new(move |router| router.layer(layer.clone())));
        self
    }
}

/// 安装 HTTP 能力：`serve` 命令、默认 middleware 和 `GET /health`。
///
/// 应用通过 `app.plugin(HttpPlugin)` 显式安装。
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

/// 根据所有已注册的路由构建最终 [`Router`]，并套用默认 middleware stack。
///
/// 通过 [`AppBuilderHttpExt::routes`] 注册的路由若在组装时 panic（例如 Axum 检测到重复路由），
/// 此函数会将其转换为启动错误 `http_route_registration_failed`。直接调用底层 Axum 构建 API
/// 不在此错误转换范围内。
pub fn build_router(app: &App) -> Result<Router> {
    validate_trusted_proxy_ranges(&app.config().server.trusted_proxy_ranges)?;
    let mut stateful = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut routes = Routes::new();
        if let Some(registry) = app.extension::<RouteRegistry>() {
            for builder in &registry.builders {
                routes = builder(routes);
            }
        }
        routes
            .into_router()
            .fallback(|| async { Error::NotFound("route not found".into()) })
            .method_not_allowed_fallback(|| async {
                Error::custom_with_status(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "method not allowed",
                )
            })
            .with_state(app.state().clone())
    }))
    .map_err(|_| {
        Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "http_route_registration_failed",
            "HTTP route registration failed; check route paths and duplicate registrations",
        )
    })?;
    if let Some(layers) = app.extension::<RouterLayerRegistry>() {
        for build_layer in &layers.builders {
            stateful = build_layer(stateful);
        }
    }
    let app_router = middleware::apply(stateful, &app.config().server, &app.config().api)?;
    Ok(app_router.layer(axum::middleware::from_fn_with_state(
        app.state().clone(),
        expose_app_state,
    )))
}

async fn expose_app_state(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(state);
    next.run(request).await
}
