//! Laravel 式受管理 HTTP 方法声明。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::handler::Handler;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{self, MethodRouter, Route};
use std::convert::Infallible;
use tower::{Layer, Service, ServiceExt};

use crate::foundation::ApplicationState;

type HandlerFuture = Pin<Box<dyn Future<Output = Response> + Send>>;
type HandlerFn = Arc<dyn Fn(Request, ApplicationState) -> HandlerFuture + Send + Sync>;
type LayerFn = Arc<dyn Fn(HandlerFn) -> HandlerFn + Send + Sync>;

#[derive(Clone)]
pub(crate) struct MiddlewareBinding {
    pub(crate) name: Option<String>,
    apply: LayerFn,
}

/// 可按 Axum 参数顺序只解析提取器、而不调用业务函数的受管理处理器。
///
/// 普通请求仍由 Axum [`Handler::call`] 执行。自然异步函数的零至十六个参数
/// 自动支持本契约；自行实现的 Axum `Handler` 需要显式实现此 trait，或使用
/// 原生 Axum Router 作为不参与 Precognition 分派的逃生口。
pub trait ManagedHandler<T, S>: Handler<T, S> {
    /// 依次解析全部参数；首个拒绝响应原样返回，成功时不执行业务函数。
    fn extract_only(
        self,
        request: Request,
        state: S,
    ) -> Pin<Box<dyn Future<Output = Result<(), Response>> + Send>>;
}

impl<F, Fut, Res, S> ManagedHandler<((),), S> for F
where
    F: FnOnce() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send,
    Res: IntoResponse,
{
    fn extract_only(
        self,
        _request: Request,
        _state: S,
    ) -> Pin<Box<dyn Future<Output = Result<(), Response>> + Send>> {
        Box::pin(async { Ok(()) })
    }
}

macro_rules! impl_managed_handler {
    ([$($ty:ident),*], $last:ident) => {
        #[allow(non_snake_case, unused_mut)]
        impl<F, Fut, S, Res, M, $($ty,)* $last> ManagedHandler<(M, $($ty,)* $last,), S> for F
        where
            F: FnOnce($($ty,)* $last,) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = Res> + Send,
            S: Send + Sync + 'static,
            Res: IntoResponse,
            $( $ty: FromRequestParts<S> + Send, )*
            $last: FromRequest<S, M> + Send,
        {
            fn extract_only(
                self,
                request: Request,
                state: S,
            ) -> Pin<Box<dyn Future<Output = Result<(), Response>> + Send>> {
                let _ = self;
                let (mut parts, body) = request.into_parts();
                Box::pin(async move {
                    $(
                        let $ty = $ty::from_request_parts(&mut parts, &state)
                            .await
                            .map_err(IntoResponse::into_response)?;
                    )*
                    let request = Request::from_parts(parts, body);
                    let $last = $last::from_request(request, &state)
                        .await
                        .map_err(IntoResponse::into_response)?;
                    let _ = ($($ty,)* $last);
                    Ok(())
                })
            }
        }
    };
}

impl_managed_handler!([], T1);
impl_managed_handler!([T1], T2);
impl_managed_handler!([T1, T2], T3);
impl_managed_handler!([T1, T2, T3], T4);
impl_managed_handler!([T1, T2, T3, T4], T5);
impl_managed_handler!([T1, T2, T3, T4, T5], T6);
impl_managed_handler!([T1, T2, T3, T4, T5, T6], T7);
impl_managed_handler!([T1, T2, T3, T4, T5, T6, T7], T8);
impl_managed_handler!([T1, T2, T3, T4, T5, T6, T7, T8], T9);
impl_managed_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9], T10);
impl_managed_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10], T11);
impl_managed_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11], T12);
impl_managed_handler!([T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12], T13);
impl_managed_handler!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13],
    T14
);
impl_managed_handler!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14],
    T15
);
impl_managed_handler!(
    [
        T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
    ],
    T16
);

fn precognitive_success() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    response.headers_mut().insert(
        "precognition-success",
        axum::http::HeaderValue::from_static("true"),
    );
    response
}

#[derive(Clone)]
pub(crate) struct MethodEntry {
    pub(crate) methods: Vec<Method>,
    pub(crate) handler: HandlerFn,
    pub(crate) metadata: Option<Arc<Mutex<crate::routing::router::MatchedRouteMetadata>>>,
    pub(crate) middleware: Vec<MiddlewareBinding>,
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
        H: ManagedHandler<T, ApplicationState>,
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
                    if crate::http::precognition::confirmed(request.extensions()) {
                        match (*handler).clone().extract_only(request, state).await {
                            Ok(()) => precognitive_success(),
                            Err(response) => response,
                        }
                    } else {
                        (*handler)
                            .clone()
                            .call(request, state)
                            .await
                            .into_response()
                    }
                })
            }),
            metadata: None,
            middleware: Vec::new(),
        });
        self
    }

    /// 在同一路径追加 GET/HEAD 处理器。
    pub fn get<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["GET"], handler)
    }
    /// 在同一路径追加 POST 处理器。
    pub fn post<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["POST"], handler)
    }
    /// 在同一路径追加 PUT 处理器。
    pub fn put<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["PUT"], handler)
    }
    /// 在同一路径追加 PATCH 处理器。
    pub fn patch<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["PATCH"], handler)
    }
    /// 在同一路径追加 DELETE 处理器。
    pub fn delete<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["DELETE"], handler)
    }
    /// 在同一路径追加 HEAD 处理器。
    pub fn head<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["HEAD"], handler)
    }
    /// 在同一路径追加 OPTIONS 处理器。
    pub fn options<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["OPTIONS"], handler)
    }
    /// 在同一路径追加 QUERY 处理器。
    pub fn query<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(&["QUERY"], handler)
    }
    /// 在同一路径追加合法 HTTP 方法列表，包括自定义方法。
    pub fn match_methods<H, T>(self, methods: &[&str], handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.append(methods, handler)
    }
    /// 在同一路径追加固定 Laravel 八种方法。
    pub fn any<H, T>(self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
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
        metadata: &Arc<Mutex<crate::routing::router::MatchedRouteMetadata>>,
    ) -> (MethodRouter<ApplicationState>, Arc<Mutex<Vec<MethodEntry>>>) {
        for entry in &mut self.entries {
            entry.metadata = Some(Arc::clone(metadata));
        }
        let entries = Arc::new(Mutex::new(self.entries));
        (materialize_shared(&entries), entries)
    }
}

pub(crate) fn materialize_shared(
    entries: &Arc<Mutex<Vec<MethodEntry>>>,
) -> MethodRouter<ApplicationState> {
    let captured = Arc::clone(entries);
    routing::any(
        move |axum::extract::State(state): axum::extract::State<ApplicationState>,
              request: Request| {
            let entries = captured
                .lock()
                .map(|entries| entries.clone())
                .map_err(|_| ());
            require_send(async move {
                match entries {
                    Ok(entries) => {
                        crate::routing::router::dispatch_managed(request, state, &entries).await
                    }
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

pub(crate) fn layer_entries<L>(entries: &Arc<Mutex<Vec<MethodEntry>>>, layer: L, name: Option<&str>)
where
    L: Layer<Route> + Clone + Send + Sync + 'static,
    L::Service: Service<Request> + Clone + Send + Sync + 'static,
    <L::Service as Service<Request>>::Response: IntoResponse + 'static,
    <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
    <L::Service as Service<Request>>::Future: Send + 'static,
{
    if let Ok(mut entries) = entries.lock() {
        for entry in entries.iter_mut() {
            let layer = layer.clone();
            entry.middleware.push(MiddlewareBinding {
                name: name.map(str::to_owned),
                apply: Arc::new(move |inner| {
                    let layer = layer.clone();
                    Arc::new(move |request, state| {
                        let inner = Arc::clone(&inner);
                        let layer = layer.clone();
                        Box::pin(async move {
                            let service = tower::service_fn(move |request| {
                                let inner = Arc::clone(&inner);
                                let state = state.clone();
                                async move { Ok::<_, Infallible>((inner)(request, state).await) }
                            });
                            let routed: MethodRouter<()> =
                                routing::any_service(service).layer(layer);
                            match routed.oneshot(request).await {
                                Ok(response) => response.into_response(),
                                Err(error) => match error {},
                            }
                        })
                    })
                }),
            });
        }
    }
}

pub(crate) fn finalize_layers(entries: &Arc<Mutex<Vec<MethodEntry>>>, priority: &[String]) {
    if let Ok(mut entries) = entries.lock() {
        for entry in entries.iter_mut() {
            let mut middleware: Vec<_> = entry.middleware.drain(..).rev().collect();
            sort_middleware(&mut middleware, priority);
            for binding in middleware.into_iter().rev() {
                entry.handler = (binding.apply)(Arc::clone(&entry.handler));
            }
        }
    }
}

fn sort_middleware(middleware: &mut Vec<MiddlewareBinding>, priority: &[String]) {
    loop {
        let mut previous: Option<(usize, usize)> = None;
        let mut moved = false;
        for index in 0..middleware.len() {
            let Some(name) = middleware[index].name.as_deref() else {
                continue;
            };
            let Some(rank) = priority.iter().position(|candidate| candidate == name) else {
                continue;
            };
            if let Some((previous_index, previous_rank)) = previous
                && rank < previous_rank
            {
                let binding = middleware.remove(index);
                middleware.insert(previous_index, binding);
                moved = true;
                break;
            }
            previous = Some((index, rank));
        }
        if !moved {
            break;
        }
    }
    let mut seen = std::collections::HashSet::new();
    middleware.retain(|binding| {
        binding
            .name
            .as_ref()
            .is_none_or(|name| seen.insert(name.clone()))
    });
}

/// 创建 GET/HEAD 声明。
pub fn get<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().get(handler)
}
/// 创建 POST 声明。
pub fn post<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().post(handler)
}
/// 创建 PUT 声明。
pub fn put<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().put(handler)
}
/// 创建 PATCH 声明。
pub fn patch<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().patch(handler)
}
/// 创建 DELETE 声明。
pub fn delete<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().delete(handler)
}
/// 创建 HEAD 声明。
pub fn head<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().head(handler)
}
/// 创建 OPTIONS 声明。
pub fn options<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().options(handler)
}
/// 创建 QUERY 声明。
pub fn query<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().query(handler)
}
/// 创建指定方法声明，包括合法自定义方法。
pub fn on<H, T>(methods: &[&str], handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().match_methods(methods, handler)
}
/// 创建固定 Laravel 八种方法声明。
pub fn any<H, T>(handler: H) -> RouteMethods
where
    H: ManagedHandler<T, ApplicationState>,
    T: 'static,
{
    RouteMethods::default().any(handler)
}
