//! gRPC server 与客户端入口，基于 tonic / Protobuf。
//!
//! 应用通过 [`GrpcPlugin`] 显式启用 gRPC；注册服务不会启动监听器，需运行 `grpc serve`。

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
    App, AppBuilder, AppState, BoxFuture, Command, Error, Result,
    config::{GrpcClientTlsConfig, GrpcConfig, GrpcServerTlsConfig},
    core::{
        CancelOnDrop, Cancellation, ExecutionContext, ExecutionMetadata, Plugin, Readiness,
        shutdown_signal,
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

type ServiceRegistration = Box<dyn Fn(&AppState, &GrpcConfig, &mut RoutesBuilder) + Send + Sync>;

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

/// 由 [`GrpcPlugin::register_service`] 收集的应用服务工厂。
#[derive(Default)]
pub struct GrpcServiceRegistry {
    names: HashSet<&'static str>,
    factories: Vec<ServiceRegistration>,
    reflection_descriptor_set: Option<Arc<[u8]>>,
    reflection_service_names: Vec<String>,
}

impl GrpcServiceRegistry {
    fn register<S, F>(&mut self, factory: F) -> Result<()>
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

    fn register_with_config<S, F>(&mut self, factory: F) -> Result<()>
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
        if !self.names.insert(S::NAME) {
            return Err(Error::custom(
                "grpc_service_duplicate",
                "同一 gRPC 服务不能重复注册",
            ));
        }
        self.factories.push(Box::new(move |state, config, routes| {
            routes.add_service(factory(state.clone(), config));
        }));
        Ok(())
    }

    fn routes(
        &self,
        state: &AppState,
        config: &GrpcConfig,
        reflection_enabled: bool,
    ) -> Result<(Routes, GrpcHealthReporter, Vec<&'static str>)> {
        if self.factories.is_empty() {
            return Err(Error::custom(
                "grpc_service_missing",
                "没有注册 gRPC 服务；请在应用构建时调用 GrpcPlugin::register_service",
            ));
        }
        let mut routes = RoutesBuilder::default();
        for factory in &self.factories {
            factory(state, config, &mut routes);
        }
        let health_services = self.names.iter().copied().collect::<Vec<_>>();
        let (health_reporter, health_service) = GrpcHealthReporter::new(&health_services);
        routes.add_service(health_service);
        if reflection_enabled {
            let descriptor_set = self.reflection_descriptor_set.as_deref().ok_or_else(|| {
                Error::custom(
                    "grpc_reflection_descriptor_missing",
                    "启用 gRPC reflection 时必须通过 GrpcPlugin::register_reflection_descriptor_set 注册应用描述符",
                )
            })?;
            if self.reflection_service_names.is_empty()
                || self
                    .reflection_service_names
                    .iter()
                    .any(|name| !self.names.contains(name.as_str()))
            {
                return Err(Error::custom(
                    "grpc_reflection_service_invalid",
                    "reflection 只能公开已注册的 gRPC 服务，且至少需要一个服务名称",
                ));
            }
            let mut reflection_builder = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(descriptor_set);
            for service_name in &self.reflection_service_names {
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

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let semaphore = Arc::clone(&self.semaphore);
        let readiness = Arc::clone(&self.readiness);
        let default_timeout = self.default_timeout;
        let max_timeout = self.max_timeout;
        Box::pin(async move {
            if !readiness.is_ready() {
                return Ok(Status::unavailable("unavailable").into_http::<Body>());
            }
            let timeout = match grpc_timeout(request.headers(), default_timeout, max_timeout) {
                Ok(timeout) => timeout,
                Err(status) => return Ok(status.into_http::<Body>()),
            };
            let Ok(permit) = semaphore.try_acquire_owned() else {
                return Ok(Status::resource_exhausted("resource_exhausted").into_http::<Body>());
            };
            let deadline = tokio::time::Instant::now() + timeout;
            let cancellation = readiness.request_cancellation();
            let execution = ExecutionContext::new(
                ExecutionMetadata::default(),
                Some(deadline),
                Cancellation::child(&cancellation),
            );
            let cancel_on_drop = CancelOnDrop::new(cancellation);
            let response = match execution.run(inner.call(request)).await {
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
                Some((Box::pin(body), permit, execution, cancel_on_drop, deadline)),
                |state| async move {
                    let (mut body, permit, execution, cancel_on_drop, deadline) = state?;
                    match tokio::time::timeout_at(deadline, execution.run(body.as_mut().frame()))
                        .await
                    {
                        Ok(Ok(Some(Ok(frame)))) => Some((
                            Ok(frame),
                            Some((body, permit, execution, cancel_on_drop, deadline)),
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

impl GrpcServer {
    /// 从服务注册表和配置构建 server；不绑定端口或启动后台任务。
    pub fn from_app(app: &App) -> Result<Self> {
        app.state().try_get::<GrpcPluginInstalled>()?;
        let registry = app.extension::<GrpcServiceRegistry>().ok_or_else(|| {
            Error::custom(
                "grpc_service_registry_missing",
                "GrpcPlugin 未注册到当前应用",
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
            readiness: app.state().try_get::<Readiness>()?,
            max_concurrent_rpcs: config.max_concurrent_rpcs,
            max_concurrent_streams_per_connection: config.max_concurrent_streams_per_connection,
            max_concurrent_rpcs_per_connection: config.max_concurrent_rpcs_per_connection,
            max_metadata_size_bytes: config.max_metadata_size_bytes,
            default_timeout: Duration::from_secs(config.default_timeout_secs),
            max_timeout: Duration::from_secs(config.max_timeout_secs),
            shutdown_timeout: Duration::from_secs(app.config().server.shutdown_timeout_secs),
            #[cfg(feature = "auth")]
            state: app.state().clone(),
        })
    }

    /// 返回配置的 listener 地址。
    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// 在调用方绑定的 TCP listener 上服务，直到 shutdown future 完成。
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
        tokio::select! {
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
                set_health_status(&health_reporter, &health_services, tonic_health::ServingStatus::NotServing).await;
                health_reporter.begin_shutdown();
                let _ = shutdown_tx.send(());
                match tokio::time::timeout(shutdown_timeout, &mut server).await {
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
        }
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
            match authenticate_grpc_bearer(&state, value).await {
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

fn read_pem(path: &Path, key: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|_| {
        Error::custom(
            "grpc_tls_material_unavailable",
            format!("{key} 指定的 PEM 文件不可读"),
        )
    })
}

/// 显式注册 gRPC 命令、客户端 channel 和应用服务。
#[derive(Default)]
pub struct GrpcPlugin {
    clients: Option<GrpcClientRegistry>,
}

#[derive(Clone, Copy)]
struct GrpcPluginInstalled;

impl GrpcPlugin {
    /// 创建配置驱动的 gRPC 插件。
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用外部构造的 channel registry，便于测试替换或应用托管连接。
    #[must_use]
    pub fn with_clients(mut self, clients: GrpcClientRegistry) -> Self {
        self.clients = Some(clients);
        self
    }

    /// 在应用构建时注册一个 tonic 生成的 server 工厂。
    ///
    /// 工厂在 `grpc serve` 构造路由时接收共享的 [`AppState`]；注册本身不会启动 listener。
    pub fn register_service<S, F>(builder: &mut AppBuilder, factory: F) -> Result<()>
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
        builder
            .extension_mut::<GrpcServiceRegistry>()
            .register(factory)
    }

    /// 注册一个可按 `[grpc]` 限额配置的 tonic 生成服务。
    ///
    /// 生成 server 必须在工厂中将 `max_decoding_message_size_bytes` 与
    /// `max_encoding_message_size_bytes` 应用到其对应构造方法。
    pub fn register_service_with_config<S, F>(builder: &mut AppBuilder, factory: F) -> Result<()>
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
        builder
            .extension_mut::<GrpcServiceRegistry>()
            .register_with_config(factory)
    }

    /// 注册应用编译时生成的 FileDescriptorSet，供显式开启的 reflection 使用。
    pub fn register_reflection_descriptor_set<I, S>(
        builder: &mut AppBuilder,
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
        let registry = builder.extension_mut::<GrpcServiceRegistry>();
        if registry.reflection_descriptor_set.is_some() {
            return Err(Error::custom(
                "grpc_reflection_descriptor_duplicate",
                "gRPC reflection 描述符只能注册一次",
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
        registry.reflection_descriptor_set = Some(Arc::from(descriptor_set));
        registry.reflection_service_names = service_names;
        Ok(())
    }
}

impl Plugin for GrpcPlugin {
    fn build(&self, builder: &mut AppBuilder) -> Result<()> {
        let config = builder.config().grpc.clone();
        config.validate()?;
        if !(1..=3600).contains(&builder.config().server.shutdown_timeout_secs) {
            return Err(Error::custom(
                "grpc_config_invalid",
                "server.shutdown_timeout_secs 必须在 1..=3600 秒之间",
            ));
        }
        if config.reflection_enabled
            && builder
                .extension_mut::<GrpcServiceRegistry>()
                .reflection_descriptor_set
                .is_none()
        {
            return Err(Error::custom(
                "grpc_reflection_descriptor_missing",
                "启用 gRPC reflection 时必须先注册应用 FileDescriptorSet",
            ));
        }
        let clients = match &self.clients {
            Some(clients) => clients.clone(),
            None => GrpcClientRegistry::from_config(&config)?,
        };
        builder.extension_mut::<GrpcServiceRegistry>();
        builder.insert(Readiness::default());
        builder.insert(GrpcPluginInstalled);
        builder.try_insert(clients)?;
        builder.try_command(GrpcServeCommand)?;
        Ok(())
    }
}

pub(crate) fn require_joint_plugin(builder: &mut AppBuilder) -> Result<()> {
    builder.try_get::<GrpcPluginInstalled>()?;
    if builder
        .extension_mut::<GrpcServiceRegistry>()
        .factories
        .is_empty()
    {
        return Err(Error::custom(
            "grpc_service_missing",
            "联合运行需要至少注册一个 gRPC 服务",
        ));
    }
    Ok(())
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
            let server = GrpcServer::from_app(app)?;
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
    use super::{GrpcClientRegistry, grpc_timeout};
    use crate::config::{GrpcClientConfig, GrpcClientTlsConfig, GrpcConfig};
    use crate::core::{Cancellation, ExecutionContext, ExecutionMetadata, with_execution_context};
    use http::{HeaderMap, HeaderValue};
    use std::collections::BTreeMap;
    use std::time::Duration;

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
