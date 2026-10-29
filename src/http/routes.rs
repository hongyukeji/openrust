//! 基于 [`axum::Router`] 的路由约定。

use std::convert::Infallible;

use crate::core::AppState;
use axum::Router;
use axum::extract::Request;
use axum::handler::Handler;
use axum::response::IntoResponse;
use axum::routing::{self, MethodRouter, Route};
use tower::{Layer, Service};

use crate::http::resource::{Resource, ResourceActions};

type GroupMiddleware = Box<dyn FnOnce(Routes) -> Routes + Send + Sync>;

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
            .push(Box::new(move |routes| routes.layer(layer)));
        self
    }

    fn apply(self, mut routes: Routes) -> Routes {
        for middleware in self.middleware.into_iter().rev() {
            routes = middleware(routes);
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
///         api.get("/posts", index).get("/posts/{id}", show)
///     })
/// }
/// ```
#[derive(Default)]
pub struct Routes {
    router: Router<AppState>,
}

impl Routes {
    /// 创建空路由表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用显式 [`MethodRouter`] 添加路由。
    #[must_use]
    pub fn route(mut self, path: &str, method_router: MethodRouter<AppState>) -> Self {
        self.router = self.router.route(path, method_router);
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
    pub fn group(mut self, group: RouteGroup, f: impl FnOnce(Routes) -> Routes) -> Self {
        let prefix = group.prefix.clone();
        let inner = group.apply(f(Routes::new())).router;
        self.router = if prefix.is_empty() || prefix == "/" {
            self.router.merge(inner)
        } else {
            self.router.nest(&prefix, inner)
        };
        self
    }

    /// 在同一层级合并另一张路由表。
    #[must_use]
    pub fn merge(mut self, other: Routes) -> Self {
        self.router = self.router.merge(other.router);
        self
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
        self.router = self.router.layer(layer);
        self
    }

    /// 将收集到的路由交给 axum。
    pub fn into_router(self) -> Router<AppState> {
        self.router
    }
}
