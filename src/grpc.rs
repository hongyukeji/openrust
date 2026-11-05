//! gRPC server 与客户端入口，基于 tonic / Protobuf。
//!
//! 应用通过 [`GrpcServiceProvider`] 显式启用 gRPC；注册服务不会启动监听器，需运行 `grpc serve`。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    future::Future,
    net::SocketAddr,
    path::Path,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

#[cfg(feature = "auth")]
use crate::auth::{CurrentUser, Policy, authenticate_grpc_bearer};
use crate::{
    App, AppState, BoxFuture, Command, Error, Result,
    config::{GrpcClientTlsConfig, GrpcConfig, GrpcServerTlsConfig},
    core::{
        CancelOnDrop, Cancellation, ExecutionContext, ExecutionMetadata, Readiness, shutdown_signal,
    },
};
use http::Request;
use http_body_util::{BodyExt, StreamBody};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
#[cfg(feature = "auth")]
use tonic::Code;
use tonic::{
    Status,
    body::Body,
    server::NamedService,
    service::{Routes, RoutesBuilder},
    transport::{Certificate, Channel, Endpoint, Identity, ServerTlsConfig, server::Router},
};
use tower::Service;

/// tonic / prost crate re-export；生成代码可通过 facade 访问运行时类型。
pub use prost;
/// tonic runtime re-export。
pub use tonic;
/// gRPC 健康检查的标准 Protobuf 消息与 typed client。
pub use tonic_health::pb as health_pb;
/// tonic-prost codec re-export，供生成代码的模块引用。
pub use tonic_prost;
/// gRPC reflection v1 的标准 Protobuf 消息与 typed client。
pub use tonic_reflection::pb as reflection_pb;

type ServiceRegistration = Arc<dyn Fn(&AppState, &GrpcConfig, &mut RoutesBuilder) + Send + Sync>;

#[cfg(feature = "metrics")]
struct GrpcCallMetricGuard {
    started: std::time::Instant,
}

#[cfg(feature = "metrics")]
impl GrpcCallMetricGuard {
    fn new() -> Self {
        metrics::counter!("openrust_grpc_calls_total").increment(1);
        metrics::gauge!("openrust_grpc_calls_in_flight").increment(1.0);
        Self {
            started: std::time::Instant::now(),
        }
    }
}

#[cfg(feature = "metrics")]
impl Drop for GrpcCallMetricGuard {
    fn drop(&mut self) {
        metrics::gauge!("openrust_grpc_calls_in_flight").decrement(1.0);
        metrics::histogram!("openrust_grpc_call_duration_seconds")
            .record(self.started.elapsed().as_secs_f64());
    }
}

#[cfg(not(feature = "metrics"))]
struct GrpcCallMetricGuard;

#[cfg(not(feature = "metrics"))]
impl GrpcCallMetricGuard {
    fn new() -> Self {
        Self
    }
}

#[derive(Clone)]
struct GrpcHealthReporter {
    statuses: Arc<
        tokio::sync::RwLock<
            HashMap<String, tokio::sync::watch::Sender<tonic_health::ServingStatus>>,
        >,
    >,
    shutting_down: tokio::sync::watch::Sender<bool>,
}

impl GrpcHealthReporter {
    fn new(
        services: &[&str],
    ) -> (
        Self,
        tonic_health::pb::health_server::HealthServer<GrpcHealthService>,
    ) {
        let statuses = std::iter::once("")
            .chain(services.iter().copied())
            .map(|name| {
                (
                    name.to_owned(),
                    tokio::sync::watch::channel(tonic_health::ServingStatus::NotServing).0,
                )
            })
            .collect();
        let statuses = Arc::new(tokio::sync::RwLock::new(statuses));
        let shutting_down = tokio::sync::watch::channel(false).0;
        let reporter = Self {
            statuses: Arc::clone(&statuses),
            shutting_down: shutting_down.clone(),
        };
        let service = GrpcHealthService {
            statuses,
            shutting_down,
        };
        (
            reporter,
            tonic_health::pb::health_server::HealthServer::new(service),
        )
    }

    async fn set_status(&self, service: &str, status: tonic_health::ServingStatus) {
        if let Some(sender) = self.statuses.read().await.get(service) {
            sender.send_replace(status);
        }
    }

    fn begin_shutdown(&self) {
        self.shutting_down.send_replace(true);
    }
}

#[derive(Clone)]
struct GrpcHealthService {
    statuses: Arc<
        tokio::sync::RwLock<
            HashMap<String, tokio::sync::watch::Sender<tonic_health::ServingStatus>>,
        >,
    >,
    shutting_down: tokio::sync::watch::Sender<bool>,
}

#[tonic::async_trait]
impl tonic_health::pb::health_server::Health for GrpcHealthService {
    async fn check(
        &self,
        request: tonic::Request<tonic_health::pb::HealthCheckRequest>,
    ) -> std::result::Result<tonic::Response<tonic_health::pb::HealthCheckResponse>, Status> {
        let statuses = self.statuses.read().await;
        let status = statuses
            .get(request.get_ref().service.as_str())
            .ok_or_else(|| Status::not_found("service not registered"))?
            .borrow()
            .to_owned();
        Ok(tonic::Response::new(
            tonic_health::pb::HealthCheckResponse {
                status: status as i32,
            },
        ))
    }

    type WatchStream = Pin<
        Box<
            dyn futures_util::Stream<
                    Item = std::result::Result<tonic_health::pb::HealthCheckResponse, Status>,
                > + Send,
        >,
    >;

    async fn watch(
        &self,
        request: tonic::Request<tonic_health::pb::HealthCheckRequest>,
    ) -> std::result::Result<tonic::Response<Self::WatchStream>, Status> {
        let statuses = self.statuses.read().await;
        let status = statuses
            .get(request.get_ref().service.as_str())
            .ok_or_else(|| Status::not_found("service not registered"))?;
        let status_receiver = status.subscribe();
        let shutdown_receiver = self.shutting_down.subscribe();
        let stream = futures_util::stream::unfold(
            Some((status_receiver, shutdown_receiver, true)),
            |state| async move {
                let (mut status_receiver, mut shutdown_receiver, first) = state?;
                if first {
                    let status = *status_receiver.borrow_and_update();
                    return Some((
                        Ok(tonic_health::pb::HealthCheckResponse {
                            status: status as i32,
                        }),
                        Some((status_receiver, shutdown_receiver, false)),
                    ));
                }
                tokio::select! {
                    biased;
                    changed = status_receiver.changed() => {
                        if changed.is_err() {
                            None
                        } else {
                            let status = *status_receiver.borrow_and_update();
                            Some((
                                Ok(tonic_health::pb::HealthCheckResponse { status: status as i32 }),
                                Some((status_receiver, shutdown_receiver, false)),
                            ))
                        }
                    }
                    changed = shutdown_receiver.changed() => {
                        if changed.is_err() || *shutdown_receiver.borrow_and_update() {
                            None
                        } else {
                            Some((
                                Err(Status::internal("health stream state changed unexpectedly")),
                                None,
                            ))
                        }
                    }
                }
            },
        );
        Ok(tonic::Response::new(Box::pin(stream)))
    }
}

/// 由 [`GrpcServiceRegistry::register`] 收集的应用服务工厂。
#[derive(Clone, Default)]
pub struct GrpcServiceRegistry {
    inner: Arc<std::sync::Mutex<GrpcRegistryInner>>,
}

#[derive(Clone, Default)]
struct GrpcRegistryInner {
    names: HashSet<&'static str>,
    factories: Vec<ServiceRegistration>,
    reflection_descriptor_set: Option<Arc<[u8]>>,
    reflection_service_names: Vec<String>,
}

impl GrpcServiceRegistry {
    /// 注册 tonic 服务工厂，重复服务名称会失败。
    pub fn register<S, F>(&mut self, factory: F) -> Result<()>
    where
        S: Service<Request<Body>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: axum::response::IntoResponse,
        S::Future: Send + 'static,
        F: Fn(AppState) -> S + Send + Sync + 'static,
    {
        self.register_with_config::<S, _>(move |state, _| factory(state))
    }

    /// 注册可读取 gRPC 配置的 tonic 服务工厂。
    pub fn register_with_config<S, F>(&mut self, factory: F) -> Result<()>
    where
        S: Service<Request<Body>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: axum::response::IntoResponse,
        S::Future: Send + 'static,
        F: Fn(AppState, &GrpcConfig) -> S + Send + Sync + 'static,
    {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !inner.names.insert(S::NAME) {
            return Err(Error::custom(
                "grpc_service_duplicate",
                "同一 gRPC 服务不能重复注册",
            ));
        }
        inner.factories.push(Arc::new(move |state, config, routes| {
            routes.add_service(factory(state.clone(), config));
        }));
        Ok(())
    }

    /// 注册应用的 reflection 描述符和服务名称。
    pub fn register_reflection_descriptor_set<I, S>(
        &mut self,
        descriptor_set: impl Into<Vec<u8>>,
        service_names: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let descriptor_set = descriptor_set.into();
        if descriptor_set.is_empty() {
            return Err(Error::custom(
                "grpc_reflection_descriptor_invalid",
                "gRPC reflection 描述符不能为空",
            ));
        }
        let service_names = service_names
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        if service_names.is_empty() || service_names.iter().any(|name| name.trim().is_empty()) {
            return Err(Error::custom(
                "grpc_reflection_service_invalid",
                "reflection 至少需要一个非空服务名称",
            ));
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.reflection_descriptor_set.is_some() {
            return Err(Error::custom(
                "grpc_reflection_descriptor_duplicate",
                "gRPC reflection 描述符只能注册一次",
            ));
        }
        inner.reflection_descriptor_set = Some(Arc::from(descriptor_set));
        inner.reflection_service_names = service_names;
        Ok(())
    }

    fn absorb(&self, other: &Self) -> Result<()> {
        if Arc::ptr_eq(&self.inner, &other.inner) {
            return Ok(());
        }
        let snapshot = other
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if snapshot.names.iter().any(|name| inner.names.contains(name)) {
            return Err(Error::custom(
                "grpc_service_duplicate",
                "同一 gRPC 服务不能重复注册",
            ));
        }
        if snapshot.reflection_descriptor_set.is_some() && inner.reflection_descriptor_set.is_some()
        {
            return Err(Error::custom(
                "grpc_reflection_descriptor_duplicate",
                "gRPC reflection 描述符只能注册一次",
            ));
        }
        inner.names.extend(snapshot.names);
        inner.factories.extend(snapshot.factories);
        if snapshot.reflection_descriptor_set.is_some() {
            inner.reflection_descriptor_set = snapshot.reflection_descriptor_set;
            inner.reflection_service_names = snapshot.reflection_service_names;
        }
        Ok(())
    }
    pub(crate) fn validate_contributions(
        &self,
        require_services: bool,
        reflection: bool,
    ) -> Result<()> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if require_services && inner.factories.is_empty() {
            return Err(Error::custom(
                "grpc_service_missing",
                "联合运行需要至少注册一个 gRPC 服务",
            ));
        }
        if reflection && inner.reflection_descriptor_set.is_none() {
            return Err(Error::custom(
                "grpc_reflection_descriptor_missing",
                "启用 gRPC reflection 时必须注册应用 FileDescriptorSet",
            ));
        }
        if reflection
            && (inner.reflection_service_names.is_empty()
                || inner
                    .reflection_service_names
                    .iter()
                    .any(|name| !inner.names.contains(name.as_str())))
        {
            return Err(Error::custom(
                "grpc_reflection_service_invalid",
                "reflection 只能公开已注册的 gRPC 服务",
            ));
        }
        let descriptor = inner.reflection_descriptor_set.clone();
        let names = inner.reflection_service_names.clone();
        drop(inner);
        if reflection {
            let descriptor = descriptor.ok_or_else(|| {
                Error::custom(
                    "grpc_reflection_descriptor_missing",
                    "启用 gRPC reflection 时必须注册应用 FileDescriptorSet",
                )
            })?;
            let mut builder = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(&descriptor);
            for name in &names {
                builder = builder.with_service_name(name);
            }
            builder.build_v1().map_err(|_| {
                Error::custom(
                    "grpc_reflection_descriptor_invalid",
                    "gRPC reflection 描述符无效",
                )
            })?;
        }
        Ok(())
    }

    fn routes(
        &self,
        state: &AppState,
        config: &GrpcConfig,
        reflection_enabled: bool,
    ) -> Result<(Routes, GrpcHealthReporter, Vec<&'static str>)> {
        let snapshot = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if snapshot.factories.is_empty() {
            return Err(Error::custom(
                "grpc_service_missing",
                "没有注册 gRPC 服务；请在应用构建时调用 GrpcServiceRegistry::register",
            ));
        }
        let mut routes = RoutesBuilder::default();
        for factory in &snapshot.factories {
            factory(state, config, &mut routes);
        }
        let health_services = snapshot.names.iter().copied().collect::<Vec<_>>();
        let (health_reporter, health_service) = GrpcHealthReporter::new(&health_services);
        routes.add_service(health_service);
        if reflection_enabled {
            let descriptor_set = snapshot.reflection_descriptor_set.as_deref().ok_or_else(|| {
                Error::custom(
                    "grpc_reflection_descriptor_missing",
                    "启用 gRPC reflection 时必须通过 GrpcServiceRegistry::register_reflection_descriptor_set 注册应用描述符",
                )
            })?;
            if snapshot.reflection_service_names.is_empty()
                || snapshot
                    .reflection_service_names
                    .iter()
                    .any(|name| !snapshot.names.contains(name.as_str()))
            {
                return Err(Error::custom(
                    "grpc_reflection_service_invalid",
                    "reflection 只能公开已注册的 gRPC 服务，且至少需要一个服务名称",
                ));
            }
            let mut reflection_builder = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(descriptor_set);
            for service_name in &snapshot.reflection_service_names {
                reflection_builder = reflection_builder.with_service_name(service_name);
            }
            let reflection = reflection_builder.build_v1().map_err(|_| {
                Error::custom(
                    "grpc_reflection_descriptor_invalid",
                    "gRPC reflection 描述符无效",
                )
            })?;
            routes.add_service(reflection);
        }
        Ok((routes.routes(), health_reporter, health_services))
    }
}

/// gRPC typed client 的共享 channel 注册表。
#[derive(Clone, Default)]
pub struct GrpcClientRegistry {
    default_client: Option<String>,
    channels: Arc<BTreeMap<String, Channel>>,
}

impl GrpcClientRegistry {
    fn from_config(config: &GrpcConfig) -> Result<Self> {
        config.validate()?;
        let mut channels = BTreeMap::new();
        for (name, client) in &config.clients {
            let mut endpoint = Endpoint::from_shared(client.endpoint.clone()).map_err(|_| {
                Error::custom(
                    "grpc_endpoint_invalid",
                    format!("grpc.clients.{name}.endpoint 不是有效的 endpoint URI"),
                )
            })?;
            if let Some(tls) = &client.tls {
                endpoint = endpoint
                    .tls_config(client_tls_config(name, tls)?)
                    .map_err(|_| {
                        Error::custom(
                            "grpc_tls_config_invalid",
                            format!("grpc.clients.{name}.tls 配置无法构造 TLS 客户端"),
                        )
                    })?;
            }
            channels.insert(name.clone(), endpoint.connect_lazy());
        }
        Ok(Self {
            default_client: config.default_client.clone(),
            channels: Arc::new(channels),
        })
    }

    /// 选择默认或具名客户端，并返回可直接传给 tonic 生成 client 的共享 Channel。
    pub fn channel(&self, name: Option<&str>) -> Result<Channel> {
        let name = name.or(self.default_client.as_deref()).ok_or_else(|| {
            Error::custom(
                "grpc_default_client_missing",
                "未配置默认 gRPC 客户端；请提供具名客户端名称",
            )
        })?;
        self.channels.get(name).cloned().ok_or_else(|| {
            Error::custom(
                "grpc_client_not_found",
                format!("未配置 gRPC 客户端 `{name}`"),
            )
        })
    }

    /// 将当前 `ExecutionContext` 的剩余预算写入 tonic 请求的 `grpc-timeout` metadata。
    ///
    /// 返回 `false` 表示当前没有带 deadline 的执行上下文，调用方可按自身策略处理。
    pub fn propagate_deadline<T>(request: &mut tonic::Request<T>) -> bool {
        let Some(remaining) =
            ExecutionContext::current().and_then(|context| context.remaining_budget())
        else {
            return false;
        };
        request.set_timeout(remaining);
        true
    }

    /// 将当前执行上下文中经过校验的 W3C `traceparent` 写入 tonic metadata。
    ///
    /// 只传播单个 trace parent；不会转发 `tracestate`、`baggage`、身份凭据或任意请求头。
    /// 返回 `false` 表示当前上下文没有 trace parent。
    pub fn propagate_trace_context<T>(request: &mut tonic::Request<T>) -> bool {
        let Some(traceparent) = ExecutionContext::current()
            .and_then(|context| context.metadata().traceparent().map(str::to_owned))
        else {
            return false;
        };
        let Ok(value) = tonic::metadata::MetadataValue::try_from(traceparent.as_str()) else {
            return false;
        };
        request.metadata_mut().insert("traceparent", value);
        true
    }

    /// 将测试或嵌入式应用提供的 Channel 替换/注册到指定名称。
    pub fn with_channel(mut self, name: impl Into<String>, channel: Channel) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(Error::custom(
                "grpc_client_name_invalid",
                "gRPC 客户端名称不能为空",
            ));
        }
        let mut channels = Arc::unwrap_or_clone(self.channels);
        channels.insert(name, channel);
        self.channels = Arc::new(channels);
        Ok(self)
    }
}

/// 绑定到已组装应用的 gRPC server 路由。
pub struct GrpcServer {
    listen_addr: SocketAddr,
    routes: Routes,
    tls: Option<ServerTlsConfig>,
    health: GrpcHealthReporter,
    health_services: Vec<&'static str>,
    readiness: Arc<Readiness>,
    max_concurrent_rpcs: usize,
    max_concurrent_streams_per_connection: u32,
    max_concurrent_rpcs_per_connection: usize,
    max_metadata_size_bytes: u32,
    default_timeout: Duration,
    max_timeout: Duration,
    shutdown_timeout: Duration,
    health_drain_grace: Duration,
    #[cfg(feature = "auth")]
    state: AppState,
}

#[derive(Clone)]
struct GrpcRuntimeLayer {
    semaphore: Arc<tokio::sync::Semaphore>,
    readiness: Arc<Readiness>,
    default_timeout: Duration,
    max_timeout: Duration,
}

impl<S> tower::Layer<S> for GrpcRuntimeLayer {
    type Service = GrpcRuntimeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcRuntimeService {
            inner,
            semaphore: Arc::clone(&self.semaphore),
            readiness: Arc::clone(&self.readiness),
            default_timeout: self.default_timeout,
            max_timeout: self.max_timeout,
        }
    }
}

#[derive(Clone)]
struct GrpcRuntimeService<S> {
    inner: S,
    semaphore: Arc<tokio::sync::Semaphore>,
    readiness: Arc<Readiness>,
    default_timeout: Duration,
    max_timeout: Duration,
}

impl<S> Service<Request<Body>> for GrpcRuntimeService<S>
where
    S: Service<Request<Body>, Response = http::Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _context: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one call owns readiness, the request budget, and its bounded response stream"
    )]
    fn call(&mut self, mut request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let semaphore = Arc::clone(&self.semaphore);
        let readiness = Arc::clone(&self.readiness);
        let default_timeout = self.default_timeout;
        let max_timeout = self.max_timeout;
        Box::pin(async move {
            let metric_guard = GrpcCallMetricGuard::new();
            let is_health_rpc = matches!(
                request.uri().path(),
                "/grpc.health.v1.Health/Check" | "/grpc.health.v1.Health/Watch"
            );
            if !readiness.is_ready() && !is_health_rpc {
                #[cfg(feature = "metrics")]
                metrics::counter!("openrust_grpc_calls_rejected_total", "reason" => "not_ready")
                    .increment(1);
                return Ok(Status::unavailable("unavailable").into_http::<Body>());
            }
            let timeout = match grpc_timeout(request.headers(), default_timeout, max_timeout) {
                Ok(timeout) => timeout,
                Err(status) => {
                    #[cfg(feature = "metrics")]
                    metrics::counter!("openrust_grpc_calls_rejected_total", "reason" => "timeout")
                        .increment(1);
                    return Ok(status.into_http::<Body>());
                }
            };
            let Ok(permit) = semaphore.try_acquire_owned() else {
                #[cfg(feature = "metrics")]
                metrics::counter!("openrust_grpc_calls_rejected_total", "reason" => "capacity")
                    .increment(1);
                return Ok(Status::resource_exhausted("resource_exhausted").into_http::<Body>());
            };
            let deadline = tokio::time::Instant::now() + timeout;
            let cancellation = readiness.request_cancellation();
            let metadata = execution_metadata_from_headers(request.headers_mut());
            let execution =
                ExecutionContext::new(metadata, Some(deadline), Cancellation::child(&cancellation));
            let cancel_on_drop = CancelOnDrop::new(cancellation);
            let span = tracing::info_span!("grpc.server");
            #[cfg(feature = "otel")]
            crate::core::otel::set_remote_parent(&span, execution.metadata().traceparent());
            let response = match execution
                .run(tracing::Instrument::instrument(
                    inner.call(request),
                    span.clone(),
                ))
                .await
            {
                Ok(Ok(response)) => response,
                Ok(Err(never)) => match never {},
                Err(_) => {
                    let status = if execution
                        .remaining_budget()
                        .is_some_and(|left| left.is_zero())
                    {
                        Status::deadline_exceeded("deadline_exceeded")
                    } else {
                        Status::cancelled("cancelled")
                    };
                    return Ok(status.into_http::<Body>());
                }
            };
            let (parts, body) = response.into_parts();
            let stream = futures_util::stream::unfold(
                Some((
                    Box::pin(body),
                    permit,
                    execution,
                    cancel_on_drop,
                    deadline,
                    span,
                    metric_guard,
                )),
                |state| async move {
                    let (mut body, permit, execution, cancel_on_drop, deadline, span, metric_guard) =
                        state?;
                    let mut body_ref = body.as_mut();
                    let frame = tracing::Instrument::instrument(
                        execution.run(body_ref.frame()),
                        span.clone(),
                    );
                    match tokio::time::timeout_at(deadline, frame).await {
                        Ok(Ok(Some(Ok(frame)))) => Some((
                            Ok(frame),
                            Some((
                                body,
                                permit,
                                execution,
                                cancel_on_drop,
                                deadline,
                                span,
                                metric_guard,
                            )),
                        )),
                        Ok(Ok(Some(Err(error)))) => Some((Err(error), None)),
                        Ok(Ok(None)) => None,
                        Ok(Err(_)) | Err(_) => {
                            let code = if execution
                                .remaining_budget()
                                .is_some_and(|left| left.is_zero())
                            {
                                "4"
                            } else {
                                "1"
                            };
                            let mut trailers = http::HeaderMap::new();
                            if let Ok(value) = http::HeaderValue::from_str(code) {
                                trailers.insert("grpc-status", value);
                            }
                            let message = if code == "4" {
                                "deadline%20exceeded"
                            } else {
                                "cancelled"
                            };
                            if let Ok(value) = http::HeaderValue::from_str(message) {
                                trailers.insert("grpc-message", value);
                            }
                            Some((Ok(http_body::Frame::trailers(trailers)), None))
                        }
                    }
                },
            );
            Ok(http::Response::from_parts(
                parts,
                Body::new(StreamBody::new(stream)),
            ))
        })
    }
}

fn execution_metadata_from_headers(headers: &mut http::HeaderMap) -> ExecutionMetadata {
    let Some(traceparent) = headers.get("traceparent") else {
        return ExecutionMetadata::default();
    };
    let Ok(traceparent) = traceparent.to_str() else {
        headers.remove("traceparent");
        return ExecutionMetadata::default();
    };
    if let Ok(metadata) = ExecutionMetadata::default().with_traceparent(traceparent.to_owned()) {
        metadata
    } else {
        headers.remove("traceparent");
        ExecutionMetadata::default()
    }
}

fn grpc_timeout(
    headers: &http::HeaderMap,
    default: Duration,
    maximum: Duration,
) -> std::result::Result<Duration, Status> {
    let mut values = headers.get_all("grpc-timeout").iter();
    let Some(value) = values.next() else {
        return Ok(default.min(maximum));
    };
    if values.next().is_some() {
        return Err(Status::invalid_argument("invalid grpc-timeout"));
    }
    let value = value
        .to_str()
        .map_err(|_| Status::invalid_argument("invalid grpc-timeout"))?;
    if value.len() < 2 || value.len() > 9 || !value.is_ascii() {
        return Err(Status::invalid_argument("invalid grpc-timeout"));
    }
    let (digits, unit) = value.split_at(value.len() - 1);
    let amount = digits
        .parse::<u64>()
        .map_err(|_| Status::invalid_argument("invalid grpc-timeout"))?;
    if digits.is_empty() || digits.len() > 8 {
        return Err(Status::invalid_argument("invalid grpc-timeout"));
    }
    let duration = match unit {
        "H" => Duration::from_secs(
            amount
                .checked_mul(3600)
                .ok_or_else(|| Status::invalid_argument("invalid grpc-timeout"))?,
        ),
        "M" => Duration::from_secs(
            amount
                .checked_mul(60)
                .ok_or_else(|| Status::invalid_argument("invalid grpc-timeout"))?,
        ),
        "S" => Duration::from_secs(amount),
        "m" => Duration::from_millis(amount),
        "u" => Duration::from_micros(amount),
        "n" => Duration::from_nanos(amount),
        _ => return Err(Status::invalid_argument("invalid grpc-timeout")),
    };
    Ok(duration.min(maximum))
}

async fn set_health_status(
    reporter: &GrpcHealthReporter,
    services: &[&str],
    status: tonic_health::ServingStatus,
) {
    reporter.set_status("", status).await;
    for service in services {
        reporter.set_status(service, status).await;
    }
}

fn validate_health_drain_grace(grace_ms: u64, shutdown_timeout_secs: u64) -> Result<()> {
    if grace_ms >= shutdown_timeout_secs.saturating_mul(1_000) {
        return Err(Error::custom(
            "grpc_config_invalid",
            "grpc.health_drain_grace_ms 必须短于 server.shutdown_timeout_secs",
        ));
    }
    Ok(())
}

impl GrpcServer {
    /// 从服务注册表和配置构建 server；不绑定端口或启动后台任务。
    pub async fn from_app(app: &App) -> Result<Self> {
        app.state().resolve::<GrpcPluginInstalled>().await?;
        let registry = app.extension::<GrpcServiceRegistry>().ok_or_else(|| {
            Error::custom(
                "grpc_service_registry_missing",
                "GrpcServiceProvider 未注册到当前应用",
            )
        })?;
        let config = &app.config().grpc;
        config.validate()?;
        if !(1..=3600).contains(&app.config().server.shutdown_timeout_secs) {
            return Err(Error::custom(
                "grpc_config_invalid",
                "server.shutdown_timeout_secs 必须在 1..=3600 秒之间",
            ));
        }
        validate_health_drain_grace(
            config.health_drain_grace_ms,
            app.config().server.shutdown_timeout_secs,
        )?;
        let (routes, health, health_services) =
            registry.routes(app.state(), config, config.reflection_enabled)?;
        if config.tls.is_none() && !config.allow_insecure_server {
            return Err(Error::custom(
                "grpc_tls_required",
                "gRPC 服务端默认要求 TLS；仅在本机开发或可信 TLS 终止拓扑显式设置 grpc.allow_insecure_server=true",
            ));
        }
        Ok(Self {
            listen_addr: config.listen_addr,
            routes,
            tls: config.tls.as_ref().map(server_tls_config).transpose()?,
            health,
            health_services,
            readiness: app.state().resolve::<Readiness>().await?,
            max_concurrent_rpcs: config.max_concurrent_rpcs,
            max_concurrent_streams_per_connection: config.max_concurrent_streams_per_connection,
            max_concurrent_rpcs_per_connection: config.max_concurrent_rpcs_per_connection,
            max_metadata_size_bytes: config.max_metadata_size_bytes,
            default_timeout: Duration::from_secs(config.default_timeout_secs),
            max_timeout: Duration::from_secs(config.max_timeout_secs),
            shutdown_timeout: Duration::from_secs(app.config().server.shutdown_timeout_secs),
            health_drain_grace: Duration::from_millis(config.health_drain_grace_ms),
            #[cfg(feature = "auth")]
            state: app.state().clone(),
        })
    }

    /// 返回配置的 listener 地址。
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// 在调用方绑定的 TCP listener 上服务，直到 shutdown future 完成。
    #[allow(
        clippy::too_many_lines,
        reason = "the server owns readiness, graceful drain, TLS, and shutdown as one lifecycle"
    )]
    pub async fn serve<F>(self, listener: TcpListener, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let GrpcServer {
            routes,
            tls,
            health,
            health_services,
            readiness,
            max_concurrent_rpcs,
            max_concurrent_streams_per_connection,
            max_concurrent_rpcs_per_connection,
            max_metadata_size_bytes,
            default_timeout,
            max_timeout,
            shutdown_timeout,
            health_drain_grace,
            #[cfg(feature = "auth")]
            state,
            ..
        } = self;
        set_health_status(
            &health,
            &health_services,
            tonic_health::ServingStatus::NotServing,
        )
        .await;
        let server = tonic::transport::Server::builder()
            .max_concurrent_streams(max_concurrent_streams_per_connection)
            .concurrency_limit_per_connection(max_concurrent_rpcs_per_connection)
            .http2_max_header_list_size(max_metadata_size_bytes)
            .load_shed(true);
        #[cfg(feature = "auth")]
        let server = server.layer(GrpcAuthLayer { state });
        let server = server.layer(GrpcRuntimeLayer {
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent_rpcs)),
            readiness: readiness.clone(),
            default_timeout,
            max_timeout,
        });
        let mut server = server;
        if let Some(tls) = tls {
            server = server.tls_config(tls).map_err(|_| {
                Error::custom("grpc_tls_config_invalid", "gRPC 服务端 TLS 配置无效")
            })?;
        }
        let router: Router<_> = server.add_routes(routes);
        let _readiness_guard = readiness.begin_listening();
        let health_reporter = health.clone();
        let readiness_for_shutdown = readiness.clone();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let mut server = tokio::spawn(async move {
            router
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
        });
        let _abort_on_drop = GrpcTaskAbortOnDrop(server.abort_handle());
        health
            .set_status("", tonic_health::ServingStatus::Serving)
            .await;
        set_health_status(
            &health,
            &health_services,
            tonic_health::ServingStatus::Serving,
        )
        .await;
        let result = tokio::select! {
            result = &mut server => {
                set_health_status(&health_reporter, &health_services, tonic_health::ServingStatus::NotServing).await;
                health_reporter.begin_shutdown();
                match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(error)) => Err(Error::internal(error)),
                    Err(error) => Err(Error::internal(error)),
                }
            }
            () = shutdown => {
                readiness_for_shutdown.stop_accepting();
                let shutdown_deadline = tokio::time::Instant::now() + shutdown_timeout;
                set_health_status(&health_reporter, &health_services, tonic_health::ServingStatus::NotServing).await;
                tokio::time::sleep_until(
                    (tokio::time::Instant::now() + health_drain_grace).min(shutdown_deadline),
                )
                .await;
                health_reporter.begin_shutdown();
                let _ = shutdown_tx.send(());
                match tokio::time::timeout_at(shutdown_deadline, &mut server).await {
                    Ok(Ok(Ok(()))) => Ok(()),
                    Ok(Ok(Err(error))) => Err(Error::internal(error)),
                    Ok(Err(error)) => Err(Error::internal(error)),
                    Err(_) => {
                        readiness_for_shutdown.cancel_requests();
                        server.abort();
                        let _ = server.await;
                        Err(Error::custom("grpc_shutdown_timeout", "gRPC server did not drain before the configured shutdown deadline"))
                    }
                }
            }
        };
        crate::core::lifecycle::flush_telemetry().await;
        result
    }
}

struct GrpcTaskAbortOnDrop(tokio::task::AbortHandle);

impl Drop for GrpcTaskAbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// gRPC handler 中读取已验证 Bearer 身份并复用框架 Policy 的辅助入口。
#[cfg(feature = "auth")]
pub struct GrpcAuth;

#[cfg(feature = "auth")]
impl GrpcAuth {
    /// 从 RPC request extension 读取由 server 验证的 JWT 或 PAT 身份。
    pub fn current_user<T>(request: &tonic::Request<T>) -> Result<&CurrentUser> {
        request
            .extensions()
            .get::<CurrentUser>()
            .ok_or(Error::Unauthorized)
    }

    /// 使用现有 Policy 和 PAT abilities 检查指定权限。
    pub async fn authorize<T>(
        request: &tonic::Request<T>,
        policy: &Policy,
        permission: &str,
    ) -> Result<()> {
        policy
            .authorize(Self::current_user(request)?, permission)
            .await
    }

    /// 将框架错误映射为不包含业务消息或内部错误来源的 gRPC Status。
    pub fn status(error: &Error) -> Status {
        status_from_error(error)
    }
}

#[cfg(feature = "auth")]
#[derive(Clone)]
struct GrpcAuthLayer {
    state: AppState,
}

#[cfg(feature = "auth")]
impl<S> tower::Layer<S> for GrpcAuthLayer {
    type Service = GrpcAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcAuthService {
            inner,
            state: self.state.clone(),
        }
    }
}

#[cfg(feature = "auth")]
#[derive(Clone)]
struct GrpcAuthService<S> {
    inner: S,
    state: AppState,
}

#[cfg(feature = "auth")]
impl<S> Service<Request<Body>> for GrpcAuthService<S>
where
    S: Service<Request<Body>, Response = http::Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _context: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let state = self.state.clone();
        Box::pin(async move {
            std::future::poll_fn(|context| inner.poll_ready(context)).await?;
            let mut values = request
                .headers()
                .get_all(http::header::AUTHORIZATION)
                .iter();
            let Some(header) = values.next() else {
                return inner.call(request).await;
            };
            if values.next().is_some() {
                return Ok(Status::unauthenticated("unauthenticated").into_http::<Body>());
            }
            let Ok(header) = header.to_str() else {
                return Ok(Status::unauthenticated("unauthenticated").into_http::<Body>());
            };
            let mut segments = header.split_ascii_whitespace();
            let scheme = segments.next().unwrap_or_default();
            let value = segments.next().unwrap_or_default();
            if !scheme.eq_ignore_ascii_case("bearer")
                || value.is_empty()
                || segments.next().is_some()
            {
                return Ok(Status::unauthenticated("unauthenticated").into_http::<Body>());
            }
            match authenticate_rpc_bearer(&state, value).await {
                Ok(user) => {
                    request.extensions_mut().insert(user);
                    inner.call(request).await
                }
                Err(error) => Ok(status_from_error(&error).into_http::<Body>()),
            }
        })
    }
}

#[cfg(feature = "auth")]
async fn authenticate_rpc_bearer(state: &AppState, value: &str) -> Result<CurrentUser> {
    #[cfg(feature = "auth-fortify")]
    if !value.starts_with("openrust_pat_")
        && state
            .container()
            .contains::<crate::auth::fortify::FortifyJwtApi>()
    {
        let scope = Arc::new(state.container().scope()?);
        return scope
            .run(async {
                scope
                    .resolve::<crate::auth::fortify::FortifyJwtApi>()
                    .await?
                    .authenticate_rpc_user(value)
                    .await
            })
            .await;
    }
    authenticate_grpc_bearer(state, value).await
}

#[cfg(feature = "auth")]
fn status_from_error(error: &Error) -> Status {
    let http_status = error.status();
    let code = if error.is_server_error() {
        Code::Internal
    } else {
        match http_status.as_u16() {
            400 | 422 => Code::InvalidArgument,
            401 => Code::Unauthenticated,
            403 => Code::PermissionDenied,
            404 => Code::NotFound,
            408 => Code::DeadlineExceeded,
            409 => Code::AlreadyExists,
            429 => Code::ResourceExhausted,
            501 => Code::Unimplemented,
            503 => Code::Unavailable,
            _ => Code::Unknown,
        }
    };
    let (message, detail) = match code {
        Code::InvalidArgument => ("invalid_argument", "invalid_argument"),
        Code::Unauthenticated => ("unauthenticated", "unauthenticated"),
        Code::PermissionDenied => ("permission_denied", "permission_denied"),
        Code::NotFound => ("not_found", "not_found"),
        Code::DeadlineExceeded => ("deadline_exceeded", "deadline_exceeded"),
        Code::AlreadyExists => ("already_exists", "already_exists"),
        Code::ResourceExhausted => ("resource_exhausted", "resource_exhausted"),
        Code::Unimplemented => ("unimplemented", "unimplemented"),
        Code::Unavailable => ("unavailable", "unavailable"),
        Code::Internal => ("internal", "internal_error"),
        _ => ("unknown", "unknown"),
    };
    Status::with_details(
        code,
        message,
        tonic::codegen::Bytes::from_static(detail.as_bytes()),
    )
}

fn server_tls_config(config: &GrpcServerTlsConfig) -> Result<ServerTlsConfig> {
    ensure_tls_crypto_provider();
    let certificate = read_pem(&config.certificate_file, "grpc.tls.certificate_file")?;
    let private_key = read_pem(&config.private_key_file, "grpc.tls.private_key_file")?;
    let identity = Identity::from_pem(certificate, private_key);
    let tls = ServerTlsConfig::new().identity(identity);
    config
        .client_ca_certificate_file
        .as_ref()
        .map(|path| {
            read_pem(path, "grpc.tls.client_ca_certificate_file")
                .map(|ca| tls.clone().client_ca_root(Certificate::from_pem(ca)))
        })
        .transpose()
        .map(|configured| configured.unwrap_or(tls))
}

fn client_tls_config(
    name: &str,
    config: &GrpcClientTlsConfig,
) -> Result<tonic::transport::ClientTlsConfig> {
    ensure_tls_crypto_provider();
    let ca = read_pem(
        &config.ca_certificate_file,
        &format!("grpc.clients.{name}.tls.ca_certificate_file"),
    )?;
    let mut tls =
        tonic::transport::ClientTlsConfig::new().ca_certificate(Certificate::from_pem(ca));
    if let (Some(certificate), Some(private_key)) = (
        config.client_certificate_file.as_ref(),
        config.client_private_key_file.as_ref(),
    ) {
        tls = tls.identity(Identity::from_pem(
            read_pem(
                certificate,
                &format!("grpc.clients.{name}.tls.client_certificate_file"),
            )?,
            read_pem(
                private_key,
                &format!("grpc.clients.{name}.tls.client_private_key_file"),
            )?,
        ));
    }
    if let Some(domain) = &config.domain_name {
        tls = tls.domain_name(domain.clone());
    }
    Ok(tls)
}

fn ensure_tls_crypto_provider() {
    // 其他启用的 HTTP/存储 feature 可能同时带入两个 rustls crypto 后端。
    // gRPC 明确选择其 tls-ring 后端；已有进程级选择则沿用。
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

fn read_pem(path: &Path, key: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|_| {
        Error::custom(
            "grpc_tls_material_unavailable",
            format!("{key} 指定的 PEM 文件不可读"),
        )
    })
}

#[derive(Clone, Copy)]
pub(crate) struct GrpcPluginInstalled;

/// 通过统一容器注册 gRPC 客户端，并贡献服务目录和命令。
#[derive(Default)]
pub struct GrpcServiceProvider {
    clients: Option<GrpcClientRegistry>,
}
impl GrpcServiceProvider {
    /// 使用配置驱动的客户端。
    pub fn new() -> Self {
        Self::default()
    }
    /// 使用调用方提供的客户端目录。
    #[must_use]
    pub fn with_clients(mut self, clients: GrpcClientRegistry) -> Self {
        self.clients = Some(clients);
        self
    }
}
impl crate::core::ServiceProvider for GrpcServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            let path = || S::String.sensitive();
            let client_tls = || {
                S::object([
                    ("ca_certificate_file", path()),
                    ("client_certificate_file", path().nullable()),
                    ("client_private_key_file", path().nullable()),
                    ("domain_name", S::String.nullable()),
                ])
                .nullable()
            };
            app.config_section(
                "grpc",
                GrpcConfig::default(),
                S::object([
                    ("listen_addr", S::String),
                    (
                        "tls",
                        S::object([
                            ("certificate_file", path()),
                            ("private_key_file", path()),
                            ("client_ca_certificate_file", path().nullable()),
                        ])
                        .nullable(),
                    ),
                    ("allow_insecure_server", S::Boolean),
                    ("max_concurrent_streams_per_connection", S::Integer),
                    ("max_concurrent_rpcs_per_connection", S::Integer),
                    ("max_concurrent_rpcs", S::Integer),
                    ("max_decoding_message_size_bytes", S::Integer),
                    ("max_encoding_message_size_bytes", S::Integer),
                    ("default_timeout_secs", S::Integer),
                    ("max_timeout_secs", S::Integer),
                    ("max_metadata_size_bytes", S::Integer),
                    ("reflection_enabled", S::Boolean),
                    ("health_drain_grace_ms", S::Integer),
                    (
                        "clients",
                        S::map(S::object([
                            ("endpoint", S::String.sensitive()),
                            ("tls", client_tls()),
                        ])),
                    ),
                    ("default_client", S::String.nullable()),
                ]),
                GrpcConfig::validate,
            )?;
            app.instance(Arc::new(GrpcPluginInstalled)).await?;
            if !app.is_bound::<GrpcServiceRegistry>() {
                app.instance(Arc::new(GrpcServiceRegistry::default()))
                    .await?;
            }
            if !app.is_bound::<Readiness>() {
                app.instance(Arc::new(Readiness::default())).await?;
            }
            let clients = self.clients.clone();
            app.singleton::<GrpcClientRegistry, _>(move |resolver| {
                let clients = clients.clone();
                Box::pin(async move {
                    let clients = match clients {
                        Some(clients) => clients,
                        None => GrpcClientRegistry::from_config(
                            &resolver.section::<GrpcConfig>("grpc")?,
                        )?,
                    };
                    Ok(Arc::new(clients))
                })
            })
            .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.section::<GrpcConfig>("grpc")?;
            let server = app.section::<crate::config::ServerConfig>("server")?;
            validate_health_drain_grace(
                config.health_drain_grace_ms,
                server.shutdown_timeout_secs,
            )?;
            app.resolve::<GrpcClientRegistry>().await?;
            let registry = app.resolve::<GrpcServiceRegistry>().await?;
            let extension = app.extension_mut::<GrpcServiceRegistry>()?;
            registry.absorb(extension)?;
            *extension = (*registry).clone();
            let config = app.config_repository();
            app.validate_at_finalize(move || {
                Box::pin(async move {
                    let grpc = config.section::<GrpcConfig>("grpc")?;
                    let server = config.section::<crate::config::ServerConfig>("server")?;
                    if !(1..=3600).contains(&server.shutdown_timeout_secs) {
                        return Err(Error::custom(
                            "grpc_config_invalid",
                            "server.shutdown_timeout_secs 必须在 1..=3600 秒之间",
                        ));
                    }
                    validate_health_drain_grace(
                        grpc.health_drain_grace_ms,
                        server.shutdown_timeout_secs,
                    )?;
                    registry.validate_contributions(false, grpc.reflection_enabled)
                })
            });
            app.command(GrpcServeCommand);
            Ok(())
        })
    }
}

struct GrpcServeCommand;

impl Command for GrpcServeCommand {
    fn name(&self) -> &'static str {
        "grpc serve"
    }

    fn about(&self) -> &'static str {
        "启动 gRPC 服务器"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let server = GrpcServer::from_app(app).await?;
            let listener = TcpListener::bind(server.listen_addr()).await?;
            let address = listener.local_addr()?;
            tracing::info!(%address, "gRPC listener ready");
            server.serve(listener, shutdown_signal()).await
        })
    }
}

/// 将生成模块与 tonic runtime 版本保持一致。
pub mod codegen {
    pub use tonic::codegen::*;
}

#[cfg(test)]
mod tests {
    use super::{GrpcClientRegistry, grpc_timeout, validate_health_drain_grace};
    use crate::config::{GrpcClientConfig, GrpcClientTlsConfig, GrpcConfig};
    use crate::core::{Cancellation, ExecutionContext, ExecutionMetadata, with_execution_context};
    use http::{HeaderMap, HeaderValue};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn health_drain_grace_must_fit_inside_shutdown_deadline() {
        assert!(validate_health_drain_grace(250, 1).is_ok());
        assert!(validate_health_drain_grace(1_000, 1).is_err());
    }

    #[test]
    fn parses_and_caps_grpc_timeout_and_rejects_ambiguous_values() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            grpc_timeout(&headers, Duration::from_secs(30), Duration::from_secs(5)).ok(),
            Some(Duration::from_secs(5))
        );
        headers.insert("grpc-timeout", HeaderValue::from_static("250m"));
        assert_eq!(
            grpc_timeout(&headers, Duration::from_secs(30), Duration::from_secs(5)).ok(),
            Some(Duration::from_millis(250))
        );
        headers.append("grpc-timeout", HeaderValue::from_static("1S"));
        assert_eq!(
            grpc_timeout(&headers, Duration::from_secs(30), Duration::from_secs(5))
                .err()
                .map(|status| status.code()),
            Some(tonic::Code::InvalidArgument)
        );
    }

    #[tokio::test]
    async fn client_deadline_propagation_uses_the_current_execution_budget() {
        let request = tonic::Request::new(());
        let mut request_without_context = request;
        assert!(!GrpcClientRegistry::propagate_deadline(
            &mut request_without_context
        ));

        let context = ExecutionContext::new(
            ExecutionMetadata::default(),
            Some(tokio::time::Instant::now() + Duration::from_secs(2)),
            Cancellation::new(),
        );
        with_execution_context(context, async {
            let mut request = tonic::Request::new(());
            assert!(GrpcClientRegistry::propagate_deadline(&mut request));
            assert!(request.metadata().get("grpc-timeout").is_some());
        })
        .await;
    }

    #[tokio::test]
    async fn client_trace_propagation_is_explicit_and_redacted() -> crate::Result<()> {
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let metadata = ExecutionMetadata::default().with_traceparent(traceparent)?;
        let context = ExecutionContext::new(metadata, None, Cancellation::new());
        with_execution_context(context, async {
            let mut request = tonic::Request::new(());
            assert!(GrpcClientRegistry::propagate_trace_context(&mut request));
            assert_eq!(
                request
                    .metadata()
                    .get("traceparent")
                    .map(tonic::metadata::MetadataValue::as_bytes),
                Some(traceparent.as_bytes())
            );
            assert!(request.metadata().get("tracestate").is_none());
            assert!(request.metadata().get("baggage").is_none());
            assert!(request.metadata().get("authorization").is_none());
            assert!(request.metadata().get("cookie").is_none());
        })
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn configured_clients_share_lazy_channels_and_validate_endpoint_scheme()
    -> crate::Result<()> {
        let config = GrpcConfig {
            clients: BTreeMap::from([
                (
                    "default".to_owned(),
                    GrpcClientConfig {
                        endpoint: "http://127.0.0.1:50051".to_owned(),
                        tls: None,
                    },
                ),
                (
                    "billing".to_owned(),
                    GrpcClientConfig {
                        endpoint: "http://127.0.0.1:50052".to_owned(),
                        tls: None,
                    },
                ),
            ]),
            default_client: Some("default".to_owned()),
            ..GrpcConfig::default()
        };
        let clients = GrpcClientRegistry::from_config(&config)?;
        assert!(clients.channel(None).is_ok());
        assert!(clients.channel(Some("billing")).is_ok());
        assert_eq!(
            clients
                .channel(Some("missing"))
                .err()
                .map(|error| error.code().to_owned()),
            Some("grpc_client_not_found".to_owned())
        );
        let tls = GrpcConfig {
            clients: BTreeMap::from([(
                "secure".to_owned(),
                GrpcClientConfig {
                    endpoint: "https://example.test".to_owned(),
                    tls: Some(GrpcClientTlsConfig {
                        ca_certificate_file: "ca.pem".into(),
                        client_certificate_file: None,
                        client_private_key_file: None,
                        domain_name: None,
                    }),
                },
            )]),
            ..GrpcConfig::default()
        };
        assert!(GrpcClientRegistry::from_config(&tls).is_err());
        Ok(())
    }

    #[cfg(feature = "auth")]
    #[test]
    fn grpc_status_mapping_uses_stable_codes_and_redacts_messages() {
        use crate::{Error, core::ValidationErrors};
        use http::StatusCode;
        use tonic::Code;

        let denied = super::GrpcAuth::status(&Error::Forbidden("private detail".to_owned()));
        assert_eq!(denied.code(), Code::PermissionDenied);
        assert_eq!(denied.message(), "permission_denied");
        assert!(!denied.details().windows(7).any(|part| part == b"private"));

        let internal = super::GrpcAuth::status(&Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database_password_leak",
            "secret connection string",
        ));
        assert_eq!(internal.code(), Code::Internal);
        assert_eq!(internal.message(), "internal");
        assert_eq!(internal.details(), b"internal_error");

        let mut fields = ValidationErrors::new();
        fields.add("password", "must not be echoed");
        let invalid = super::GrpcAuth::status(&Error::Validation(fields));
        assert_eq!(invalid.code(), Code::InvalidArgument);
        assert_eq!(invalid.details(), b"invalid_argument");
    }
}

#[cfg(all(test, feature = "auth-fortify", feature = "db-sqlite"))]
pub(crate) mod jwt_profile_tests {
    use super::{AppState, GrpcAuth, GrpcAuthLayer, Policy, ensure_tls_crypto_provider};
    use crate::{Error, Result};
    use std::time::Duration;
    use tonic::transport::{
        Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Server, ServerTlsConfig,
    };
    use tonic::{Code, Request, Response, Status};
    use tonic_health::pb::{
        HealthCheckRequest, HealthCheckResponse,
        health_client::HealthClient,
        health_server::{Health, HealthServer},
    };

    struct Probe(Policy);
    #[tonic::async_trait]
    impl Health for Probe {
        type WatchStream =
            futures_util::stream::Empty<std::result::Result<HealthCheckResponse, Status>>;

        async fn check(
            &self,
            request: Request<HealthCheckRequest>,
        ) -> std::result::Result<Response<HealthCheckResponse>, Status> {
            if request.get_ref().service == "protected" {
                let user =
                    GrpcAuth::current_user(&request).map_err(|error| GrpcAuth::status(&error))?;
                if user.id() != 42 || user.source() != crate::auth::AuthSource::Guard {
                    return Err(Status::permission_denied("unexpected identity"));
                }
                GrpcAuth::authorize(&request, &self.0, "rpc:read")
                    .await
                    .map_err(|error| GrpcAuth::status(&error))?;
            }
            Ok(Response::new(HealthCheckResponse {
                status: tonic_health::ServingStatus::Serving as i32,
            }))
        }

        async fn watch(
            &self,
            _: Request<HealthCheckRequest>,
        ) -> std::result::Result<Response<Self::WatchStream>, Status> {
            Err(Status::unimplemented("unary probe"))
        }
    }

    fn request(service: &str, token: Option<&str>) -> Result<Request<HealthCheckRequest>> {
        let mut request = Request::new(HealthCheckRequest {
            service: service.into(),
        });
        if let Some(token) = token {
            request.metadata_mut().insert(
                "authorization",
                format!("Bearer {token}").parse().map_err(Error::internal)?,
            );
        }
        Ok(request)
    }

    async fn verify_client(channel: Channel, valid: &str, denied: &[String]) -> Result<()> {
        let mut client = HealthClient::new(channel);
        client
            .check(request("public", None)?)
            .await
            .map_err(Error::internal)?;
        assert!(
            client
                .check(request("protected", None)?)
                .await
                .is_err_and(|status| status.code() == Code::Unauthenticated)
        );
        client
            .check(request("protected", Some(valid))?)
            .await
            .map_err(Error::internal)?;
        for token in denied {
            // 公共方法收到无效显式凭据也必须在进入 handler 前失败。
            assert!(
                client
                    .check(request("public", Some(token))?)
                    .await
                    .is_err_and(|status| status.code() == Code::Unauthenticated)
            );
        }
        Ok(())
    }

    pub(crate) async fn verify_over_tls(
        state: AppState,
        policy: Policy,
        valid: &str,
        denied: &[String],
    ) -> Result<()> {
        ensure_tls_crypto_provider();
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .map_err(Error::internal)?;
        let pem = certified.cert.pem();
        let identity = Identity::from_pem(pem.as_bytes(), certified.signing_key.serialize_pem());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Endpoint::from_shared(format!("https://{}", listener.local_addr()?))
            .map_err(Error::internal)?
            .tls_config(
                ClientTlsConfig::new()
                    .domain_name("localhost")
                    .ca_certificate(Certificate::from_pem(pem)),
            )
            .map_err(Error::internal)?;
        let (stop, signal) = tokio::sync::oneshot::channel::<()>();
        let server = Box::pin(
            Server::builder()
                .tls_config(ServerTlsConfig::new().identity(identity))
                .map_err(Error::internal)?
                .layer(GrpcAuthLayer { state })
                .add_service(HealthServer::new(Probe(policy)))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    async {
                        let _ = signal.await;
                    },
                ),
        );
        let client = async {
            let channel = endpoint.connect().await.map_err(Error::internal)?;
            let result = verify_client(channel, valid, denied).await;
            drop(stop);
            result
        };
        tokio::time::timeout(Duration::from_secs(20), async {
            tokio::try_join!(async { server.await.map_err(Error::internal) }, client)
        })
        .await
        .map_err(Error::internal)??;
        Ok(())
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::{BootContext, ProviderBuilder, ServiceProvider};
    use figment::{Figment, providers::Serialized};
    struct InvalidOverlay;
    impl ServiceProvider for InvalidOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(
                async move { app.overlay("grpc", serde_json::json!({"max_concurrent_rpcs": 0})) },
            )
        }
    }
    #[tokio::test]
    async fn provider_binds_shared_clients_and_command() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(GrpcServiceProvider::new());
        let runtime = builder
            .start(Figment::from(Serialized::defaults(
                crate::config::FrameworkConfig::default(),
            )))
            .await?;
        let a = runtime.container().resolve::<GrpcClientRegistry>().await?;
        let b = runtime.container().resolve::<GrpcClientRegistry>().await?;
        assert!(Arc::ptr_eq(&a, &b));
        assert!(
            runtime
                .commands()
                .any(|command| command.name() == "grpc serve")
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }
    struct LateGrace;
    impl ServiceProvider for LateGrace {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay("grpc", serde_json::json!({"health_drain_grace_ms": 60_000}))
            })
        }
    }
    #[tokio::test]
    async fn finalizer_rejects_cross_section_grace() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(GrpcServiceProvider::new())
            .provider(LateGrace);
        assert!(
            builder
                .start(Figment::from(Serialized::defaults(
                    crate::config::FrameworkConfig::default()
                )))
                .await
                .is_err()
        );
    }
    #[test]
    fn reflection_iterator_runs_without_registry_lock() -> Result<()> {
        let mut registry = GrpcServiceRegistry::default();
        let shared = registry.clone();
        let mut emitted = false;
        let names = std::iter::from_fn(move || {
            if emitted {
                return None;
            }
            emitted = true;
            assert!(shared.inner.try_lock().is_ok());
            Some("test.Service")
        });
        registry.register_reflection_descriptor_set(vec![1], names)
    }
    #[test]
    fn invalid_reflection_descriptor_fails_before_ready() -> Result<()> {
        let mut registry = GrpcServiceRegistry::default();
        registry.register_reflection_descriptor_set(vec![0xff], ["test.Service"])?;
        registry
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .names
            .insert("test.Service");
        let error = registry
            .validate_contributions(false, true)
            .err()
            .ok_or_else(|| Error::custom("test", "invalid descriptor accepted"))?;
        assert_eq!(error.code(), "grpc_reflection_descriptor_invalid");
        Ok(())
    }
    #[test]
    fn registry_clones_share_late_contributions() -> Result<()> {
        let registry = GrpcServiceRegistry::default();
        let clone = registry.clone();
        assert!(clone.validate_contributions(true, false).is_err());
        {
            let mut inner = registry
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.names.insert("test.Service");
            inner.factories.push(Arc::new(|_, _, _| {}));
        }
        clone.validate_contributions(true, false)
    }
    #[tokio::test]
    async fn provider_rejects_late_invalid_limits() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(GrpcServiceProvider::new())
            .provider(InvalidOverlay);
        assert!(
            builder
                .start(Figment::from(Serialized::defaults(
                    crate::config::FrameworkConfig::default()
                )))
                .await
                .is_err()
        );
    }
}
