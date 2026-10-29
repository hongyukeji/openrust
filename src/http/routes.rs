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
    name_prefix: String,
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
///         api.get("/posts", index).name("posts.index").get("/posts/{id}", show)
///     })
/// }
/// ```
#[derive(Default)]
pub struct Routes {
    router: Router<AppState>,
    last_route: Option<(String, String)>,
    route_names: Vec<(String, String)>,
    route_name_errors: Vec<String>,
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
        self.last_route = Some((path.to_owned(), String::new()));
        self
    }

    /// 为最近注册的路由指定唯一名称，供 [`crate::http::RouteUrlGenerator`] 生成 URL。
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if let Some((path, prefix)) = self.last_route.take() {
            self.route_names.push((format!("{prefix}{name}"), path));
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
    pub fn group(mut self, group: RouteGroup, f: impl FnOnce(Routes) -> Routes) -> Self {
        let prefix = group.prefix.clone();
        let name_prefix = group.name_prefix.clone();
        let mut inner = group.apply(f(Routes::new()));
        for (name, path) in &mut inner.route_names {
            *name = format!("{name_prefix}{name}");
            *path = join_route_path(&prefix, path);
        }
        if let Some((path, pending_name_prefix)) = &mut inner.last_route {
            *path = join_route_path(&prefix, path);
            *pending_name_prefix = format!("{name_prefix}{pending_name_prefix}");
        }
        self.router = if prefix.is_empty() || prefix == "/" {
            self.router.merge(inner.router)
        } else {
            self.router.nest(&prefix, inner.router)
        };
        self.route_names.extend(inner.route_names);
        self.route_name_errors.extend(inner.route_name_errors);
        self.last_route = inner.last_route;
        self
    }

    /// 在同一层级合并另一张路由表。
    #[must_use]
    pub fn merge(mut self, other: Routes) -> Self {
        self.router = self.router.merge(other.router);
        self.route_names.extend(other.route_names);
        self.route_name_errors.extend(other.route_name_errors);
        self.last_route = other.last_route;
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

    pub(crate) fn into_parts(self) -> (Router<AppState>, Vec<(String, String)>, Vec<String>) {
        (self.router, self.route_names, self.route_name_errors)
    }
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
