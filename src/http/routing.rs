//! Laravel 式受管理 HTTP 方法声明。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::Request;
use axum::handler::Handler;
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::routing::{self, MethodRouter, Route};
use std::convert::Infallible;
use tower::{Layer, Service, ServiceExt};

use crate::core::AppState;

type HandlerFuture = Pin<Box<dyn Future<Output = Response> + Send>>;
type HandlerFn = Arc<dyn Fn(Request, AppState) -> HandlerFuture + Send + Sync>;

#[derive(Clone)]
pub(crate) struct MethodEntry {
    pub(crate) methods: Vec<Method>,
    pub(crate) handler: HandlerFn,
    pub(crate) metadata: Option<Arc<Mutex<super::routes::MatchedRouteMetadata>>>,
}

/// 一条路径的受管理方法和处理器声明；最终仍由 Axum 匹配路径。
#[derive(Clone, Default)]
#[must_use]
pub struct RouteMethods {
    pub(crate) entries: Vec<MethodEntry>,
    pub(crate) errors: Vec<String>,
}

impl RouteMethods {
    pub(crate) fn merge(mut self, other: Self) -> Self {
        self.entries.extend(other.entries);
        self.errors.extend(other.errors);
        self
    }
    fn append<H, T>(mut self, methods: &[&str], handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        let parsed: Option<Vec<Method>> = methods
            .iter()
            .map(|method| {
                let normalized = method.to_ascii_uppercase();
                Method::from_bytes(normalized.as_bytes()).ok()
            })
            .collect();
        let Some(mut parsed) = parsed else {
            self.errors.push("invalid HTTP method token".to_owned());
            return self;
        };
        if parsed.is_empty() {
            self.errors
                .push("HTTP method declaration is empty".to_owned());
            return self;
        }
        if parsed.contains(&Method::GET) && !parsed.contains(&Method::HEAD) {
            parsed.push(Method::HEAD);
        }
        let handler = Arc::new(handler);
        self.entries.push(MethodEntry {
            methods: parsed,
            handler: Arc::new(move |request, state| {
                let handler = Arc::clone(&handler);
                Box::pin(async move {
                    (*handler)
                        .clone()
                        .call(request, state)
                        .await
                        .into_response()
                })
            }),
            metadata: None,
        });
        self
    }

    /// 在同一路径追加 GET/HEAD 处理器。
    pub fn get<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["GET"], handler)
    }
    /// 在同一路径追加 POST 处理器。
    pub fn post<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["POST"], handler)
    }
    /// 在同一路径追加 PUT 处理器。
    pub fn put<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["PUT"], handler)
    }
    /// 在同一路径追加 PATCH 处理器。
    pub fn patch<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["PATCH"], handler)
    }
    /// 在同一路径追加 DELETE 处理器。
    pub fn delete<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["DELETE"], handler)
    }
    /// 在同一路径追加 HEAD 处理器。
    pub fn head<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["HEAD"], handler)
    }
    /// 在同一路径追加 OPTIONS 处理器。
    pub fn options<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["OPTIONS"], handler)
    }
    /// 在同一路径追加 QUERY 处理器。
    pub fn query<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(&["QUERY"], handler)
    }
    /// 在同一路径追加合法 HTTP 方法列表，包括自定义方法。
    pub fn match_methods<H, T>(self, methods: &[&str], handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(methods, handler)
    }
    /// 在同一路径追加固定 Laravel 八种方法。
    pub fn any<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.append(
            &[
                "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY",
            ],
            handler,
        )
    }

    pub(crate) fn materialize(
        mut self,
        metadata: &Arc<Mutex<super::routes::MatchedRouteMetadata>>,
    ) -> (MethodRouter<AppState>, Arc<Mutex<Vec<MethodEntry>>>) {
        for entry in &mut self.entries {
            entry.metadata = Some(Arc::clone(metadata));
        }
        let entries = Arc::new(Mutex::new(self.entries));
        (materialize_shared(&entries), entries)
    }
}

pub(crate) fn materialize_shared(entries: &Arc<Mutex<Vec<MethodEntry>>>) -> MethodRouter<AppState> {
    let captured = Arc::clone(entries);
    routing::any(
        move |axum::extract::State(state): axum::extract::State<AppState>, request: Request| {
            let entries = captured
                .lock()
                .map(|entries| entries.clone())
                .map_err(|_| ());
            require_send(async move {
                match entries {
                    Ok(entries) => super::routes::dispatch_managed(request, state, &entries).await,
                    Err(()) => crate::core::Error::custom(
                        "http_route_context_poisoned",
                        "请求路由上下文不可用",
                    )
                    .into_response(),
                }
            })
        },
    )
}

fn require_send<F: Future + Send>(future: F) -> F {
    future
}

pub(crate) fn layer_entries<L>(entries: &Arc<Mutex<Vec<MethodEntry>>>, layer: L)
where
    L: Layer<Route> + Clone + Send + Sync + 'static,
    L::Service: Service<Request> + Clone + Send + Sync + 'static,
    <L::Service as Service<Request>>::Response: IntoResponse + 'static,
    <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
    <L::Service as Service<Request>>::Future: Send + 'static,
{
    if let Ok(mut entries) = entries.lock() {
        for entry in entries.iter_mut() {
            let inner = Arc::clone(&entry.handler);
            let layer = layer.clone();
            entry.handler = Arc::new(move |request, state| {
                let inner = Arc::clone(&inner);
                let layer = layer.clone();
                Box::pin(async move {
                    let service = tower::service_fn(move |request| {
                        let inner = Arc::clone(&inner);
                        let state = state.clone();
                        async move { Ok::<_, Infallible>((inner)(request, state).await) }
                    });
                    let routed: MethodRouter<()> = routing::any_service(service).layer(layer);
                    match routed.oneshot(request).await {
                        Ok(response) => response.into_response(),
                        Err(error) => match error {},
                    }
                })
            });
        }
    }
}

/// 创建 GET/HEAD 声明。
pub fn get<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().get(handler)
}
/// 创建 POST 声明。
pub fn post<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().post(handler)
}
/// 创建 PUT 声明。
pub fn put<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().put(handler)
}
/// 创建 PATCH 声明。
pub fn patch<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().patch(handler)
}
/// 创建 DELETE 声明。
pub fn delete<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().delete(handler)
}
/// 创建 HEAD 声明。
pub fn head<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().head(handler)
}
/// 创建 OPTIONS 声明。
pub fn options<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().options(handler)
}
/// 创建 QUERY 声明。
pub fn query<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().query(handler)
}
/// 创建指定方法声明，包括合法自定义方法。
pub fn on<H, T>(methods: &[&str], handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().match_methods(methods, handler)
}
/// 创建固定 Laravel 八种方法声明。
pub fn any<H, T>(handler: H) -> RouteMethods
where
    H: Handler<T, AppState>,
    T: 'static,
{
    RouteMethods::default().any(handler)
}
