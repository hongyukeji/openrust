//! 具有明确资源限制的共享出站 HTTP 客户端。

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use http::{HeaderMap, Method, StatusCode};
use ipnet::IpNet;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::de::DeserializeOwned;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// [`HttpRequest::body`] 接受的请求正文类型。
pub use reqwest::Body as HttpBody;
/// 提供给 [`HttpTransport`] 实现的请求类型。
pub use reqwest::Request as HttpTransportRequest;
/// [`HttpRequest::multipart`] 接受的 multipart 表单正文类型。
pub use reqwest::multipart::Form as MultipartForm;

/// [`HttpClient`] 的资源和超时设置。
#[derive(Clone, PartialEq, Eq)]
pub struct HttpClientConfig {
    /// 建立连接的最长等待时间。
    pub connect_timeout: Duration,
    /// 完整请求与响应正文的最长处理时间。
    pub timeout: Duration,
    /// [`HttpResponse::bytes`] 最多缓冲的字节数。
    pub max_response_bytes: usize,
    /// 可选的显式代理 URL。环境代理变量会被忽略。
    pub proxy: Option<String>,
}

/// 经 [`HttpClient::restricted`] 启用的出站目标约束。
#[derive(Debug, Clone)]
pub struct HttpTargetPolicy {
    allowed_hosts: Option<HashSet<String>>,
    allowed_ports: HashSet<u16>,
    internal_networks: Vec<(String, IpNet, HashSet<u16>)>,
}

impl Default for HttpTargetPolicy {
    fn default() -> Self {
        Self {
            allowed_hosts: Some(HashSet::new()),
            allowed_ports: HashSet::from([80, 443]),
            internal_networks: Vec::new(),
        }
    }
}

impl HttpTargetPolicy {
    /// 默认拒绝全部主机；登记后只允许指定主机访问公网 HTTP/HTTPS 默认端口。
    pub fn new() -> Self {
        Self::default()
    }

    /// 将请求限制到给定的精确主机名或 IP 字面量。
    pub fn allow_host(&mut self, host: impl Into<String>) -> Result<&mut Self, HttpClientError> {
        let host = normalize_host(&host.into()).ok_or(HttpClientError::InvalidConfig)?;
        self.allowed_hosts
            .get_or_insert_with(HashSet::new)
            .insert(host);
        Ok(self)
    }

    /// 允许该主机连接一个内网网段及指定端口；同一主机不能重复登记。
    pub fn allow_internal(
        &mut self,
        host: impl Into<String>,
        network: IpNet,
        ports: impl IntoIterator<Item = u16>,
    ) -> Result<&mut Self, HttpClientError> {
        let host = normalize_host(&host.into()).ok_or(HttpClientError::InvalidConfig)?;
        let ports: HashSet<_> = ports.into_iter().collect();
        if ports.is_empty()
            || ports.contains(&0)
            || self
                .internal_networks
                .iter()
                .any(|(allowed_host, _, _)| allowed_host == &host)
        {
            return Err(HttpClientError::InvalidConfig);
        }
        self.allowed_hosts
            .get_or_insert_with(HashSet::new)
            .insert(host.clone());
        self.internal_networks.push((host, network, ports));
        Ok(self)
    }

    /// 允许额外的公网 HTTP/HTTPS 端口。
    pub fn allow_port(&mut self, port: u16) -> Result<&mut Self, HttpClientError> {
        if port == 0 {
            return Err(HttpClientError::InvalidConfig);
        }
        self.allowed_ports.insert(port);
        Ok(self)
    }

    fn permits_host(&self, host: &str) -> bool {
        self.allowed_hosts
            .as_ref()
            .is_none_or(|allowed| allowed.contains(host))
    }

    fn permits_port(&self, host: &str, port: u16) -> bool {
        let has_internal_rule = self
            .internal_networks
            .iter()
            .any(|(allowed_host, _, _)| allowed_host == host);
        if has_internal_rule {
            self.internal_networks
                .iter()
                .any(|(allowed_host, _, ports)| allowed_host == host && ports.contains(&port))
        } else {
            self.allowed_ports.contains(&port)
        }
    }

    fn permits_address(&self, host: &str, port: u16, address: IpAddr) -> bool {
        is_public_address(address)
            || self
                .internal_networks
                .iter()
                .any(|(allowed_host, network, ports)| {
                    allowed_host == host && ports.contains(&port) && network.contains(&address)
                })
    }

    fn permits_resolved_address(&self, host: &str, address: IpAddr) -> bool {
        is_public_address(address)
            || self
                .internal_networks
                .iter()
                .any(|(allowed_host, network, _)| {
                    allowed_host == host && network.contains(&address)
                })
    }
}

impl std::fmt::Debug for HttpClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpClientConfig")
            .field("connect_timeout", &self.connect_timeout)
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("proxy", &self.proxy.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(30),
            max_response_bytes: 1024 * 1024,
            proxy: None,
        }
    }
}

/// 出站 HTTP 客户端构建、传输或正文大小限制错误。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpClientError {
    /// 发生超时、网络故障或协议故障；详情会被主动隐藏。
    #[error("outbound HTTP transport failed")]
    Transport,
    /// 已达到配置的连接超时或请求总超时。
    #[error("outbound HTTP request timed out")]
    Timeout,
    /// 响应正文超过配置的大小上限。
    #[error("outbound HTTP response exceeded the configured body limit")]
    BodyTooLarge,
    /// 响应正文无法解码为 JSON。
    #[error("outbound HTTP response contained invalid JSON")]
    InvalidJson,
    /// 客户端配置无效。
    #[error("invalid outbound HTTP client configuration")]
    InvalidConfig,
    /// 请求正文是一次性 stream，因此无法重放请求。
    #[error("outbound HTTP request body cannot be replayed")]
    UnreplayableRequest,
    /// 请求目标不符合受限出站策略。
    #[error("outbound HTTP target rejected by policy")]
    TargetRejected,
    /// 命名依赖的并发等待队列已满。
    #[error("outbound dependency queue is full")]
    DependencyOverloaded,
    /// 命名依赖的熔断器处于打开状态。
    #[error("outbound dependency circuit is open")]
    CircuitOpen,
}

/// 执行出站请求的传输层。实现不得记录请求中的秘密信息。
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// 发送一次请求并返回流式响应。
    async fn send(
        &self,
        request: HttpTransportRequest,
    ) -> Result<TransportResponse, HttpClientError>;
}

/// [`HttpTransport`] 使用的响应正文来源。
#[async_trait]
pub trait HttpResponseBody: Send {
    /// 返回一个正文块；到达 stream 末尾后返回 `None`。
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError>;
}

/// 框架应用已配置的正文大小上限前的传输响应。
pub struct TransportResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Box<dyn HttpResponseBody>,
}

impl TransportResponse {
    /// 根据元数据和正文来源创建响应。
    pub fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: impl HttpResponseBody + 'static,
    ) -> Self {
        Self {
            status,
            headers,
            body: Box::new(body),
        }
    }

    /// 创建内存响应正文，便于使用确定性的 fake transport。
    pub fn from_bytes(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self::new(status, headers, BytesResponseBody(Some(body)))
    }
}

struct BytesResponseBody(Option<Vec<u8>>);

#[async_trait]
impl HttpResponseBody for BytesResponseBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
        Ok(self.0.take())
    }
}

struct ReqwestTransport(reqwest::Client);

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn send(&self, request: reqwest::Request) -> Result<TransportResponse, HttpClientError> {
        let response = self
            .0
            .execute(request)
            .await
            .map_err(|error| map_transport_error(&error))?;
        let status = response.status();
        let headers = response.headers().clone();
        Ok(TransportResponse::new(
            status,
            headers,
            ReqwestResponseBody(response),
        ))
    }
}

struct ReqwestResponseBody(reqwest::Response);

#[async_trait]
impl HttpResponseBody for ReqwestResponseBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
        self.0
            .chunk()
            .await
            .map(|chunk| chunk.map(|chunk| chunk.to_vec()))
            .map_err(|error| map_transport_error(&error))
    }
}

/// 调用方声明请求具备幂等性后使用的有界重试策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u8,
    base_delay: Duration,
    max_delay: Duration,
    retry_after_cap: Duration,
    jitter: bool,
}

impl RetryPolicy {
    /// 创建尝试次数为 1 到 5 次、指数退避时间有上限的策略。
    pub fn new(
        max_attempts: u8,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Result<Self, HttpClientError> {
        if !(1..=5).contains(&max_attempts) || base_delay > max_delay {
            return Err(HttpClientError::InvalidConfig);
        }
        Ok(Self {
            max_attempts,
            base_delay,
            max_delay,
            retry_after_cap: max_delay,
            jitter: true,
        })
    }

    /// 设置 `Retry-After` 可延长退避的最大时长；超过总时限时仍由总时限优先截断。
    #[must_use]
    pub fn retry_after_cap(mut self, cap: Duration) -> Self {
        self.retry_after_cap = cap;
        self
    }

    /// 设置是否对指数退避应用全抖动。
    #[must_use]
    pub fn jitter(mut self, enabled: bool) -> Self {
        self.jitter = enabled;
        self
    }

    fn delay(self, retry_index: u32) -> Duration {
        let ceiling = self
            .base_delay
            .saturating_mul(2_u32.saturating_pow(retry_index))
            .min(self.max_delay);
        if !self.jitter || ceiling.is_zero() {
            return ceiling;
        }
        let nanos = u64::try_from(ceiling.as_nanos().min(u128::from(u64::MAX))).unwrap_or(u64::MAX);
        Duration::from_nanos(jitter_value(nanos))
    }
}

/// 单个命名出站依赖的容量和可选熔断设置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyPolicy {
    /// 同时执行的逻辑请求数。
    pub max_in_flight: usize,
    /// 等待并发许可的最大请求数；超出时立即拒绝。
    pub max_waiting: usize,
    /// 可选的连续失败熔断策略。
    pub circuit_breaker: Option<CircuitBreakerPolicy>,
}

impl Default for DependencyPolicy {
    fn default() -> Self {
        Self {
            max_in_flight: 32,
            max_waiting: 64,
            circuit_breaker: None,
        }
    }
}

/// 可选熔断器的阈值、冷却时间和半开探测并发上限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitBreakerPolicy {
    /// 打开熔断器所需的连续失败数。
    pub failure_threshold: u32,
    /// 熔断打开后进入半开的等待时间。
    pub reset_timeout: Duration,
    /// 半开状态允许的并行探测数。
    pub half_open_max_calls: usize,
}

/// 客户端克隆间共享的命名依赖注册表。
type DependencyRegistry = Arc<Mutex<HashMap<String, (DependencyPolicy, Arc<DependencyRuntime>)>>>;

/// 可复用的出站客户端。克隆实例共享底层连接池。
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    transport: Arc<dyn HttpTransport>,
    max_response_bytes: usize,
    timeout: Duration,
    target_policy: Option<Arc<HttpTargetPolicy>>,
    dependencies: DependencyRegistry,
}

struct DependencyRuntime {
    semaphore: Arc<Semaphore>,
    waiting: AtomicUsize,
    max_waiting: usize,
    breaker: Option<(CircuitBreakerPolicy, Mutex<BreakerState>)>,
}

#[derive(Default)]
struct BreakerState {
    failures: u32,
    opened_at: Option<tokio::time::Instant>,
    half_open_calls: usize,
}

/// 共享使用同一命名依赖隔离状态的 HTTP 入口。
#[derive(Clone)]
pub struct DependencyClient {
    name: Arc<str>,
    client: HttpClient,
    runtime: Arc<DependencyRuntime>,
}

impl HttpClient {
    /// 获取按名称共享容量与熔断状态的出站依赖入口。
    pub fn dependency(
        &self,
        name: impl Into<String>,
        policy: DependencyPolicy,
    ) -> Result<DependencyClient, HttpClientError> {
        let name = name.into();
        if name.trim().is_empty()
            || policy.max_in_flight == 0
            || policy.circuit_breaker.is_some_and(|breaker| {
                breaker.failure_threshold == 0
                    || breaker.reset_timeout.is_zero()
                    || breaker.half_open_max_calls == 0
            })
        {
            return Err(HttpClientError::InvalidConfig);
        }
        let mut dependencies = self
            .dependencies
            .lock()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        let runtime = if let Some((existing, runtime)) = dependencies.get(&name) {
            if existing != &policy {
                return Err(HttpClientError::InvalidConfig);
            }
            Arc::clone(runtime)
        } else {
            let runtime = Arc::new(DependencyRuntime {
                semaphore: Arc::new(Semaphore::new(policy.max_in_flight)),
                waiting: AtomicUsize::new(0),
                max_waiting: policy.max_waiting,
                breaker: policy
                    .circuit_breaker
                    .map(|breaker| (breaker, Mutex::new(BreakerState::default()))),
            });
            dependencies.insert(name.clone(), (policy, Arc::clone(&runtime)));
            runtime
        };
        Ok(DependencyClient {
            name: Arc::from(name),
            client: self.clone(),
            runtime,
        })
    }
}

impl DependencyClient {
    /// 返回此入口对应的依赖名称。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回底层客户端是否启用了 [`HttpClient::restricted`] 目标策略。
    pub fn is_restricted(&self) -> bool {
        self.client.target_policy.is_some()
    }

    /// 创建依赖隔离范围内的 GET 请求。
    pub fn get(&self, url: &str) -> DependencyRequest {
        DependencyRequest {
            dependency: self.clone(),
            request: self.client.get(url),
        }
    }

    /// 创建依赖隔离范围内的 POST 请求。
    pub fn post(&self, url: &str) -> DependencyRequest {
        DependencyRequest {
            dependency: self.clone(),
            request: self.client.post(url),
        }
    }

    /// 创建依赖隔离范围内的通用请求。
    pub fn request(&self, method: Method, url: &str) -> DependencyRequest {
        DependencyRequest {
            dependency: self.clone(),
            request: self.client.request(method, url),
        }
    }

    async fn acquire(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<DependencyPermit, HttpClientError> {
        self.check_breaker()?;
        let permit = if let Ok(permit) = Arc::clone(&self.runtime.semaphore).try_acquire_owned() {
            permit
        } else {
            let queued =
                self.runtime
                    .waiting
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |waiting| {
                        (waiting < self.runtime.max_waiting).then_some(waiting + 1)
                    });
            if queued.is_err() {
                return Err(HttpClientError::DependencyOverloaded);
            }
            let waiter = QueueWaiter(&self.runtime.waiting);
            let acquired = tokio::time::timeout_at(
                deadline,
                Arc::clone(&self.runtime.semaphore).acquire_owned(),
            )
            .await;
            drop(waiter);
            acquired
                .map_err(|_| HttpClientError::Timeout)?
                .map_err(|_| HttpClientError::DependencyOverloaded)?
        };
        self.reserve_breaker_probe()?;
        Ok(DependencyPermit {
            dependency: self.clone(),
            _permit: permit,
            outcome_recorded: false,
        })
    }

    fn check_breaker(&self) -> Result<(), HttpClientError> {
        let Some((policy, state)) = &self.runtime.breaker else {
            return Ok(());
        };
        let state = state.lock().map_err(|_| HttpClientError::CircuitOpen)?;
        if let Some(opened_at) = state.opened_at {
            if opened_at.elapsed() < policy.reset_timeout {
                return Err(HttpClientError::CircuitOpen);
            }
            if state.half_open_calls >= policy.half_open_max_calls {
                return Err(HttpClientError::CircuitOpen);
            }
        }
        Ok(())
    }

    fn reserve_breaker_probe(&self) -> Result<(), HttpClientError> {
        let Some((policy, state)) = &self.runtime.breaker else {
            return Ok(());
        };
        let mut state = state.lock().map_err(|_| HttpClientError::CircuitOpen)?;
        if let Some(opened_at) = state.opened_at {
            if opened_at.elapsed() < policy.reset_timeout
                || state.half_open_calls >= policy.half_open_max_calls
            {
                return Err(HttpClientError::CircuitOpen);
            }
            state.half_open_calls += 1;
        }
        Ok(())
    }

    fn record_outcome(&self, failed: bool) {
        let Some((policy, state)) = &self.runtime.breaker else {
            return;
        };
        let Ok(mut state) = state.lock() else {
            return;
        };
        if failed {
            state.failures = state.failures.saturating_add(1);
            state.half_open_calls = state.half_open_calls.saturating_sub(1);
            if state.failures >= policy.failure_threshold {
                state.opened_at = Some(tokio::time::Instant::now());
            }
        } else {
            state.failures = 0;
            state.opened_at = None;
            state.half_open_calls = 0;
        }
    }
}

/// 处于指定命名依赖容量与熔断范围内的请求。
pub struct DependencyRequest {
    dependency: DependencyClient,
    request: HttpRequest,
}

impl DependencyRequest {
    /// 限制本次调用时限；不会延长客户端配置的总请求时限。
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request.timeout = self.request.timeout.min(timeout);
        self
    }

    /// 设置 JSON 请求正文。
    #[must_use]
    pub fn json<T: serde::Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.request = self.request.json(value);
        self
    }

    /// 设置 URL 编码的表单正文。
    #[must_use]
    pub fn form<T: serde::Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.request = self.request.form(value);
        self
    }

    /// 设置 multipart 表单正文。
    #[must_use]
    pub fn multipart(mut self, form: MultipartForm) -> Self {
        self.request = self.request.multipart(form);
        self
    }

    /// 设置原始请求正文。
    #[must_use]
    pub fn body(mut self, body: HttpBody) -> Self {
        self.request = self.request.body(body);
        self
    }

    /// 设置请求头。
    #[must_use]
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.request = self.request.headers(headers);
        self
    }

    /// 添加查询参数。
    #[must_use]
    pub fn query<T: serde::Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.request = self.request.query(value);
        self
    }

    /// 设置 bearer authorization 值。
    #[must_use]
    pub fn bearer_auth(mut self, token: &str) -> Self {
        self.request = self.request.bearer_auth(token);
        self
    }

    /// 发送请求；排队时间计入客户端总时限。
    pub async fn send(self) -> Result<HttpResponse, HttpClientError> {
        let deadline = tokio::time::Instant::now() + self.request.timeout;
        let permit = self.dependency.acquire(deadline).await?;
        let mut request = self.request;
        request.timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        let result = match tokio::time::timeout_at(deadline, request.send()).await {
            Ok(result) => result,
            Err(_) => Err(HttpClientError::Timeout),
        };
        finish_dependency_response(permit, result)
    }

    /// 将请求声明为可安全重试，并在同一容量许可下执行所有尝试。
    pub fn idempotent(self) -> IdempotentDependencyRequest {
        IdempotentDependencyRequest {
            dependency: self.dependency,
            request: self.request.idempotent(),
        }
    }
}

/// 已声明幂等且受命名依赖隔离的请求。
pub struct IdempotentDependencyRequest {
    dependency: DependencyClient,
    request: IdempotentHttpRequest,
}

impl IdempotentDependencyRequest {
    /// 在总时限内排队并执行受限重试。
    pub async fn send_with_retry(
        self,
        policy: RetryPolicy,
    ) -> Result<HttpResponse, HttpClientError> {
        let timeout = self.request.0.timeout;
        let deadline = tokio::time::Instant::now() + timeout;
        let permit = self.dependency.acquire(deadline).await?;
        let mut request = self.request;
        request.0.timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        let result = match tokio::time::timeout_at(deadline, request.send_with_retry(policy)).await
        {
            Ok(result) => result,
            Err(_) => Err(HttpClientError::Timeout),
        };
        finish_dependency_response(permit, result)
    }
}

fn finish_dependency_response(
    mut permit: DependencyPermit,
    result: Result<HttpResponse, HttpClientError>,
) -> Result<HttpResponse, HttpClientError> {
    permit.record(&result);
    result.map(|mut response| {
        response.dependency_permit = Some(permit);
        response
    })
}

struct DependencyPermit {
    dependency: DependencyClient,
    _permit: OwnedSemaphorePermit,
    outcome_recorded: bool,
}

struct QueueWaiter<'a>(&'a AtomicUsize);

impl Drop for QueueWaiter<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl DependencyPermit {
    fn record(&mut self, result: &Result<HttpResponse, HttpClientError>) {
        let outcome = match result {
            Ok(response) => Some(response.status().is_server_error()),
            Err(HttpClientError::Transport | HttpClientError::Timeout) => Some(true),
            Err(_) => None,
        };
        if let Some(failed) = outcome {
            self.dependency.record_outcome(failed);
            self.outcome_recorded = true;
        }
    }

    fn record_stream_failure(&self) {
        self.dependency.record_outcome(true);
    }
}

impl Drop for DependencyPermit {
    fn drop(&mut self) {
        if self.outcome_recorded {
            return;
        }
        if let Some((_, state)) = &self.dependency.runtime.breaker
            && let Ok(mut state) = state.lock()
            && state.opened_at.is_some()
        {
            state.half_open_calls = state.half_open_calls.saturating_sub(1);
        }
    }
}

fn jitter_value(ceiling: u64) -> u64 {
    use std::sync::atomic::AtomicU64;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);
    let mut value = SEQUENCE.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    value ^= value >> 12;
    value ^= value << 25;
    value ^= value >> 27;
    value.wrapping_mul(0x2545_f491_4f6c_dd1d) % ceiling.saturating_add(1)
}

impl HttpClient {
    /// 创建已校验资源限制且禁用自动重定向的客户端。
    pub fn new(config: HttpClientConfig) -> Result<Self, HttpClientError> {
        validate_config(&config)?;
        let mut builder = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        if let Some(proxy) = config.proxy {
            let proxy = reqwest::Proxy::all(proxy).map_err(|_| HttpClientError::InvalidConfig)?;
            builder = builder.proxy(proxy);
        }
        let inner = builder
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        Ok(Self {
            inner: inner.clone(),
            transport: Arc::new(ReqwestTransport(inner)),
            max_response_bytes: config.max_response_bytes,
            timeout: config.timeout,
            target_policy: None,
            dependencies: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 创建启用目标校验、DNS 固定和禁止代理的受限客户端。
    #[allow(
        clippy::needless_pass_by_value,
        reason = "与 HttpClient::new 保持一致的公开构造器接收配置值"
    )]
    pub fn restricted(
        config: HttpClientConfig,
        policy: HttpTargetPolicy,
    ) -> Result<Self, HttpClientError> {
        Self::restricted_with_resolver(&config, policy, Arc::new(SystemResolver))
    }

    fn restricted_with_resolver(
        config: &HttpClientConfig,
        policy: HttpTargetPolicy,
        resolver: Arc<dyn Resolve>,
    ) -> Result<Self, HttpClientError> {
        validate_config(config)?;
        if config.proxy.is_some() {
            return Err(HttpClientError::InvalidConfig);
        }
        let policy = Arc::new(policy);
        let inner = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(RestrictedResolver {
                policy: Arc::clone(&policy),
                resolver,
            })
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        Ok(Self {
            inner: inner.clone(),
            transport: Arc::new(ReqwestTransport(inner)),
            max_response_bytes: config.max_response_bytes,
            timeout: config.timeout,
            target_policy: Some(policy),
            dependencies: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 创建使用注入传输层的客户端，可用于测试 fake 或应用自定义传输层。
    #[allow(
        clippy::needless_pass_by_value,
        reason = "configuration is consumed at the client construction boundary"
    )]
    pub fn with_transport(
        config: HttpClientConfig,
        transport: impl HttpTransport + 'static,
    ) -> Result<Self, HttpClientError> {
        validate_config(&config)?;
        let inner = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        Ok(Self {
            inner,
            transport: Arc::new(transport),
            max_response_bytes: config.max_response_bytes,
            timeout: config.timeout,
            target_policy: None,
            dependencies: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 发起请求。丢弃返回的 future 会取消本地等待，但无法撤销远程服务器已经处理的请求。
    pub fn request(&self, method: Method, url: &str) -> HttpRequest {
        HttpRequest {
            inner: self.inner.request(method, url),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: url_has_userinfo(url),
        }
    }

    /// 发起 GET 请求。
    pub fn get(&self, url: &str) -> HttpRequest {
        self.request(Method::GET, url)
    }

    /// 发起 POST 请求。
    pub fn post(&self, url: &str) -> HttpRequest {
        self.request(Method::POST, url)
    }
}

struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Addrs = Box::new(
                tokio::net::lookup_host((host.as_str(), 0))
                    .await?
                    .collect::<Vec<_>>()
                    .into_iter(),
            );
            Ok(addresses)
        })
    }
}

struct RestrictedResolver {
    policy: Arc<HttpTargetPolicy>,
    resolver: Arc<dyn Resolve>,
}

impl Resolve for RestrictedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host =
            normalize_host(name.as_str()).unwrap_or_else(|| name.as_str().to_ascii_lowercase());
        let resolving = self.resolver.resolve(name);
        let policy = Arc::clone(&self.policy);
        Box::pin(async move {
            if !policy.permits_host(&host) {
                return Err(Box::new(std::io::Error::other("target rejected")) as _);
            }
            let addresses: Vec<SocketAddr> = resolving.await?.collect();
            if addresses.is_empty()
                || addresses
                    .iter()
                    .any(|address| !policy.permits_resolved_address(&host, address.ip()))
            {
                return Err(Box::new(std::io::Error::other("target rejected")) as _);
            }
            let addresses: Addrs = Box::new(addresses.into_iter());
            Ok(addresses)
        })
    }
}

fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim_end_matches('.');
    if let Ok(address) = IpAddr::from_str(host) {
        return Some(address.to_string());
    }
    if host.is_empty() || host.contains(['/', '@', ':']) {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn validate_target(
    request: &reqwest::Request,
    policy: &HttpTargetPolicy,
) -> Result<(), HttpClientError> {
    let url = request.url();
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || request.headers().contains_key(http::header::HOST)
    {
        return Err(HttpClientError::TargetRejected);
    }
    let host = url
        .host_str()
        .and_then(normalize_host)
        .ok_or(HttpClientError::TargetRejected)?;
    let port = url
        .port_or_known_default()
        .ok_or(HttpClientError::TargetRejected)?;
    if !policy.permits_host(&host) || !policy.permits_port(&host, port) {
        return Err(HttpClientError::TargetRejected);
    }
    if let Ok(address) = IpAddr::from_str(&host)
        && !policy.permits_address(&host, port, address)
    {
        return Err(HttpClientError::TargetRejected);
    }
    Ok(())
}

fn url_has_userinfo(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        if !url.username().is_empty() || url.password().is_some() {
            return true;
        }
        let Some(authority) = value.split_once("://").map(|(_, rest)| rest) else {
            return false;
        };
        authority
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    })
}

fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    let in_range = |network: [u8; 4], prefix: u8| {
        let value = u32::from(address);
        let base = u32::from(Ipv4Addr::from(network));
        let mask = u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0);
        value & mask == base & mask
    };
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_private()
        && !address.is_link_local()
        && !address.is_multicast()
        && !address.is_broadcast()
        && !in_range([0, 0, 0, 0], 8)
        && !in_range([100, 64, 0, 0], 10)
        && !in_range([192, 0, 0, 0], 24)
        && !in_range([192, 0, 2, 0], 24)
        && !in_range([192, 88, 99, 0], 24)
        && !in_range([198, 18, 0, 0], 15)
        && !in_range([198, 51, 100, 0], 24)
        && !in_range([203, 0, 113, 0], 24)
        && octets[0] < 240
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if let Some(ipv4) = address.to_ipv4_mapped() {
        return is_public_ipv4(ipv4);
    }
    let segments = address.segments();
    (segments[0] & 0xe000 == 0x2000)
        && !in_ipv6_prefix(address, "2001:db8::", 32)
        && !in_ipv6_prefix(address, "2001::", 23)
        && !in_ipv6_prefix(address, "2002::", 16)
        && !in_ipv6_prefix(address, "3fff::", 20)
}

fn in_ipv6_prefix(address: Ipv6Addr, network: &str, prefix: u8) -> bool {
    Ipv6Addr::from_str(network).is_ok_and(|network| {
        let value = u128::from(address);
        let base = u128::from(network);
        let mask = u128::MAX.checked_shl(u32::from(128 - prefix)).unwrap_or(0);
        value & mask == base & mask
    })
}

fn map_transport_error(error: &reqwest::Error) -> HttpClientError {
    if error.is_timeout() {
        HttpClientError::Timeout
    } else {
        HttpClientError::Transport
    }
}

fn validate_config(config: &HttpClientConfig) -> Result<(), HttpClientError> {
    if config.connect_timeout.is_zero()
        || config.timeout.is_zero()
        || config.max_response_bytes == 0
    {
        return Err(HttpClientError::InvalidConfig);
    }
    if let Some(proxy) = &config.proxy {
        reqwest::Proxy::all(proxy).map_err(|_| HttpClientError::InvalidConfig)?;
    }
    Ok(())
}

/// 处于构建阶段、支持常用正文编码的请求。
pub struct HttpRequest {
    inner: reqwest::RequestBuilder,
    transport: Arc<dyn HttpTransport>,
    max_response_bytes: usize,
    timeout: Duration,
    target_policy: Option<Arc<HttpTargetPolicy>>,
    target_precheck_failed: bool,
}

impl HttpRequest {
    /// 设置 JSON 请求正文。
    #[must_use]
    pub fn json<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            inner: self.inner.json(value),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 设置 URL 编码的表单正文。
    #[must_use]
    pub fn form<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            inner: self.inner.form(value),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 设置 multipart 请求正文。
    #[must_use]
    pub fn multipart(self, form: MultipartForm) -> Self {
        Self {
            inner: self.inner.multipart(form),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 设置原始请求正文，也可以使用异步 stream 正文。
    #[must_use]
    pub fn body(self, body: HttpBody) -> Self {
        Self {
            inner: self.inner.body(body),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 设置请求头。
    #[must_use]
    pub fn headers(self, headers: HeaderMap) -> Self {
        Self {
            inner: self.inner.headers(headers),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 将序列化后的查询参数添加到请求 URL。
    #[must_use]
    pub fn query<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            inner: self.inner.query(value),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 设置 bearer authorization 值。
    #[must_use]
    pub fn bearer_auth(self, token: &str) -> Self {
        Self {
            inner: self.inner.bearer_auth(token),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
            target_policy: self.target_policy.clone(),
            target_precheck_failed: self.target_precheck_failed,
        }
    }

    /// 发送请求并返回状态码和响应头，不受状态码类别影响。
    pub async fn send(self) -> Result<HttpResponse, HttpClientError> {
        let transport = self.transport;
        let max_response_bytes = self.max_response_bytes;
        let target_policy = self.target_policy;
        let target_precheck_failed = self.target_precheck_failed;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let request = self
            .inner
            .build()
            .map_err(|error| map_transport_error(&error))?;
        if let Some(policy) = &target_policy {
            if target_precheck_failed {
                return Err(HttpClientError::TargetRejected);
            }
            validate_target(&request, policy)?;
        }
        let response = tokio::time::timeout_at(deadline, transport.send(request))
            .await
            .map_err(|_| HttpClientError::Timeout)??;
        HttpResponse::from_transport(response, max_response_bytes, deadline)
    }

    /// 声明调用方可以安全地重放此操作。
    ///
    /// 这是应用作出的保证；仅根据 HTTP 方法名称无法判断某个远程操作是否具备幂等性。
    #[must_use]
    pub fn idempotent(self) -> IdempotentHttpRequest {
        IdempotentHttpRequest(self)
    }
}

/// 调用方明确声明可安全重试的请求。
pub struct IdempotentHttpRequest(HttpRequest);

impl IdempotentHttpRequest {
    /// 发生传输故障或收到 5xx 状态码时，使用有界指数退避发送请求。
    pub async fn send_with_retry(
        self,
        policy: RetryPolicy,
    ) -> Result<HttpResponse, HttpClientError> {
        let HttpRequest {
            inner,
            transport,
            max_response_bytes,
            timeout,
            target_policy,
            target_precheck_failed,
        } = self.0;
        let request = inner.build().map_err(|error| map_transport_error(&error))?;
        if let Some(policy) = &target_policy {
            if target_precheck_failed {
                return Err(HttpClientError::TargetRejected);
            }
            validate_target(&request, policy)?;
        }
        let deadline = tokio::time::Instant::now() + timeout;
        if policy.max_attempts == 1 {
            let response = tokio::time::timeout_at(deadline, transport.send(request))
                .await
                .map_err(|_| HttpClientError::Timeout)??;
            return HttpResponse::from_transport(response, max_response_bytes, deadline);
        }
        if policy.max_attempts > 1 && request.try_clone().is_none() {
            return Err(HttpClientError::UnreplayableRequest);
        }
        for attempt in 0..policy.max_attempts {
            let request = request
                .try_clone()
                .ok_or(HttpClientError::UnreplayableRequest)?;
            let response = tokio::time::timeout_at(deadline, transport.send(request))
                .await
                .map_err(|_| HttpClientError::Timeout)?;
            match response {
                Ok(response)
                    if attempt + 1 < policy.max_attempts
                        && matches!(
                            response.status,
                            StatusCode::TOO_MANY_REQUESTS
                                | StatusCode::BAD_GATEWAY
                                | StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::GATEWAY_TIMEOUT
                        ) =>
                {
                    let retry_after = retry_after_delay(&response.headers, policy);
                    drop(response);
                    let delay = retry_after
                        .map_or(policy.delay(u32::from(attempt)), |server_delay| {
                            policy.delay(u32::from(attempt)).max(server_delay)
                        });
                    tokio::time::timeout_at(deadline, tokio::time::sleep(delay))
                        .await
                        .map_err(|_| HttpClientError::Timeout)?;
                }
                Ok(response) => {
                    return HttpResponse::from_transport(response, max_response_bytes, deadline);
                }
                Err(error)
                    if attempt + 1 < policy.max_attempts
                        && matches!(
                            error,
                            HttpClientError::Transport | HttpClientError::Timeout
                        ) =>
                {
                    tokio::time::timeout_at(
                        deadline,
                        tokio::time::sleep(policy.delay(u32::from(attempt))),
                    )
                    .await
                    .map_err(|_| HttpClientError::Timeout)?;
                }
                Err(error) => return Err(error),
            }
        }
        Err(HttpClientError::Transport)
    }
}

fn retry_after_delay(headers: &HeaderMap, policy: RetryPolicy) -> Option<Duration> {
    let value = headers.get(http::header::RETRY_AFTER)?.to_str().ok()?;
    let delay = value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value).ok().map(|date| {
                date.duration_since(std::time::SystemTime::now())
                    .unwrap_or_default()
            })
        })?;
    Some(delay.min(policy.retry_after_cap).min(policy.max_delay))
}

impl HttpResponse {
    fn from_transport(
        response: TransportResponse,
        max_response_bytes: usize,
        deadline: tokio::time::Instant,
    ) -> Result<Self, HttpClientError> {
        if let Some(content_length) = response.headers.get(http::header::CONTENT_LENGTH) {
            let content_length = content_length
                .to_str()
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or(HttpClientError::Transport)?;
            if content_length > max_response_bytes {
                return Err(HttpClientError::BodyTooLarge);
            }
        }
        Ok(Self {
            status: response.status,
            headers: response.headers,
            body: response.body,
            max_response_bytes,
            body_bytes_read: 0,
            deadline,
            dependency_permit: None,
        })
    }
}

/// 响应元数据和带大小限制的正文读取器。
pub struct HttpResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Box<dyn HttpResponseBody>,
    max_response_bytes: usize,
    body_bytes_read: usize,
    deadline: tokio::time::Instant,
    dependency_permit: Option<DependencyPermit>,
}

impl HttpResponse {
    /// 服务器返回的 HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// 响应头。
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// 读取一个响应块；到达 stream 末尾后返回 `None`。
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
        let next_chunk = tokio::time::timeout_at(self.deadline, self.body.next_chunk())
            .await
            .map_err(|_| HttpClientError::Timeout)
            .and_then(|result| result);
        let chunk = match next_chunk {
            Ok(Some(chunk)) => chunk,
            Ok(None) => {
                self.dependency_permit.take();
                return Ok(None);
            }
            Err(error) => {
                if let Some(permit) = &self.dependency_permit {
                    permit.record_stream_failure();
                }
                self.dependency_permit.take();
                return Err(error);
            }
        };
        let new_size = self.body_bytes_read.saturating_add(chunk.len());
        if new_size > self.max_response_bytes {
            if let Some(permit) = &self.dependency_permit {
                permit.record_stream_failure();
            }
            self.dependency_permit.take();
            return Err(HttpClientError::BodyTooLarge);
        }
        self.body_bytes_read = new_size;
        Ok(Some(chunk[..].to_vec()))
    }

    /// 读取响应正文，并强制执行配置的大小上限。
    pub async fn bytes(mut self) -> Result<Vec<u8>, HttpClientError> {
        let mut body = Vec::new();
        while let Some(chunk) = self.next_chunk().await? {
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// 在配置的大小限制内读取并反序列化 JSON 响应。
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, HttpClientError> {
        let body = self.bytes().await?;
        serde_json::from_slice(&body).map_err(|_| HttpClientError::InvalidJson)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::Json;
    use axum::routing::get;
    use reqwest::dns::Resolve;
    use serde::Deserialize;
    use tokio::net::TcpListener;

    use super::{
        HttpClient, HttpClientConfig, HttpClientError, HttpResponseBody, HttpTargetPolicy,
        HttpTransport, IpNet, RestrictedResolver, RetryPolicy, TransportResponse,
        is_public_address, retry_after_delay,
    };

    struct FakeTransport {
        responses: Mutex<VecDeque<Result<http::StatusCode, HttpClientError>>>,
        requests: AtomicUsize,
        content_types: Mutex<Vec<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl HttpTransport for FakeTransport {
        async fn send(
            &self,
            request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            assert_eq!(request.url().host_str(), Some("fake.invalid"));
            self.requests.fetch_add(1, Ordering::SeqCst);
            let content_type = request
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            self.content_types
                .lock()
                .map_err(|_| HttpClientError::Transport)?
                .push(content_type);
            let status = self
                .responses
                .lock()
                .map_err(|_| HttpClientError::Transport)?
                .pop_front()
                .unwrap_or(Ok(http::StatusCode::OK))?;
            Ok(TransportResponse::from_bytes(
                status,
                http::HeaderMap::new(),
                br#"{"value":"fake"}"#.to_vec(),
            ))
        }
    }

    #[derive(Deserialize)]
    struct Message {
        value: String,
    }

    #[tokio::test]
    async fn local_http_json_and_body_limit() -> Result<(), Box<dyn std::error::Error>> {
        let app = axum::Router::new()
            .route(
                "/json",
                get(|| async { Json(serde_json::json!({"value": "ok"})) }),
            )
            .route(
                "/redirect",
                get(|| async { (http::StatusCode::FOUND, [(http::header::LOCATION, "/json")]) }),
            )
            .route("/large", get(|| async { "123456789" }));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let client = HttpClient::new(HttpClientConfig::default())?;

        let response = client.get(&format!("http://{address}/json")).send().await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        let redirect = client
            .get(&format!("http://{address}/redirect"))
            .send()
            .await?;
        assert_eq!(redirect.status(), http::StatusCode::FOUND);
        let message: Message = client
            .get(&format!("http://{address}/json"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(message.value, "ok");

        let mut streamed = client
            .get(&format!("http://{address}/large"))
            .send()
            .await?;
        assert!(streamed.next_chunk().await?.is_some());
        assert!(streamed.next_chunk().await?.is_none());

        let limited = HttpClient::new(HttpClientConfig {
            max_response_bytes: 4,
            ..HttpClientConfig::default()
        })?;
        assert!(matches!(
            limited.get(&format!("http://{address}/large")).send().await,
            Err(HttpClientError::BodyTooLarge)
        ));
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn invalid_timeout_configuration_is_rejected() {
        let config = HttpClientConfig {
            timeout: Duration::ZERO,
            ..HttpClientConfig::default()
        };
        assert!(matches!(
            HttpClient::new(config),
            Err(HttpClientError::InvalidConfig)
        ));
    }

    #[tokio::test]
    async fn breaker_does_not_count_local_policy_rejections_as_dependency_failures()
    -> Result<(), HttpClientError> {
        let client = HttpClient::new(HttpClientConfig::default())?;
        let dependency = client.dependency(
            "catalog",
            super::DependencyPolicy {
                max_in_flight: 1,
                max_waiting: 1,
                circuit_breaker: Some(super::CircuitBreakerPolicy {
                    failure_threshold: 1,
                    reset_timeout: Duration::from_secs(1),
                    half_open_max_calls: 1,
                }),
            },
        )?;
        let Some((_, breaker)) = &dependency.runtime.breaker else {
            return Err(HttpClientError::InvalidConfig);
        };
        breaker
            .lock()
            .map_err(|_| HttpClientError::InvalidConfig)?
            .failures = 1;
        let mut permit = dependency
            .acquire(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        permit.record(&Err(HttpClientError::TargetRejected));
        assert_eq!(
            breaker
                .lock()
                .map_err(|_| HttpClientError::InvalidConfig)?
                .failures,
            1
        );
        Ok(())
    }

    #[test]
    fn retry_after_values_are_parsed_and_capped() -> Result<(), HttpClientError> {
        let policy = RetryPolicy::new(3, Duration::from_millis(10), Duration::from_secs(4))?
            .retry_after_cap(Duration::from_secs(2));
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_static("30"),
        );
        assert_eq!(
            retry_after_delay(&headers, policy),
            Some(Duration::from_secs(2))
        );
        headers.insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_static("invalid"),
        );
        assert_eq!(retry_after_delay(&headers, policy), None);
        Ok(())
    }

    #[test]
    fn retry_jitter_stays_within_the_exponential_ceiling() -> Result<(), HttpClientError> {
        let policy = RetryPolicy::new(3, Duration::from_millis(10), Duration::from_millis(25))?;
        for index in 0..100 {
            assert!(policy.delay(1) <= Duration::from_millis(20));
            assert!(policy.delay(index % 3) <= Duration::from_millis(25));
        }
        Ok(())
    }

    #[test]
    fn configuration_debug_redacts_proxy_credentials() {
        let config = HttpClientConfig {
            proxy: Some("http://user:secret@proxy.example".to_owned()),
            ..HttpClientConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("REDACTED"));
    }

    #[tokio::test]
    async fn total_timeout_is_enforced() -> Result<(), Box<dyn std::error::Error>> {
        let app = axum::Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                "late"
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let client = HttpClient::new(HttpClientConfig {
            timeout: Duration::from_millis(10),
            ..HttpClientConfig::default()
        })?;
        assert!(matches!(
            client.get(&format!("http://{address}/slow")).send().await,
            Err(HttpClientError::Timeout)
        ));
        server.abort();
        Ok(())
    }

    struct SlowTransport;

    #[async_trait::async_trait]
    impl HttpTransport for SlowTransport {
        async fn send(
            &self,
            _request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(TransportResponse::from_bytes(
                http::StatusCode::OK,
                http::HeaderMap::new(),
                Vec::new(),
            ))
        }
    }

    #[tokio::test]
    async fn injected_transport_respects_the_configured_timeout() -> Result<(), HttpClientError> {
        let client = HttpClient::with_transport(
            HttpClientConfig {
                timeout: Duration::from_millis(10),
                ..HttpClientConfig::default()
            },
            SlowTransport,
        )?;
        assert!(matches!(
            client.get("http://fake.invalid/slow").send().await,
            Err(HttpClientError::Timeout)
        ));
        Ok(())
    }

    struct SlowResponseBody;

    #[async_trait::async_trait]
    impl HttpResponseBody for SlowResponseBody {
        async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(None)
        }
    }

    struct SlowBodyTransport;

    #[async_trait::async_trait]
    impl HttpTransport for SlowBodyTransport {
        async fn send(
            &self,
            _request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            Ok(TransportResponse::new(
                http::StatusCode::OK,
                http::HeaderMap::new(),
                SlowResponseBody,
            ))
        }
    }

    #[tokio::test]
    async fn total_timeout_covers_injected_response_body_reads() -> Result<(), HttpClientError> {
        let client = HttpClient::with_transport(
            HttpClientConfig {
                timeout: Duration::from_millis(10),
                ..HttpClientConfig::default()
            },
            SlowBodyTransport,
        )?;
        let mut response = client.get("http://fake.invalid/slow-body").send().await?;
        assert!(matches!(
            response.next_chunk().await,
            Err(HttpClientError::Timeout)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn fake_transport_never_opens_a_network_connection() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([Ok(http::StatusCode::OK)])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let response = client.get("http://fake.invalid/endpoint").send().await?;
        let message: Message = response.json().await?;
        assert_eq!(message.value, "fake");
        assert_eq!(transport.requests.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn fake_body_without_content_length_is_still_bounded() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([Ok(http::StatusCode::OK)])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig {
                max_response_bytes: 4,
                ..HttpClientConfig::default()
            },
            ArcTransport(transport),
        )?;
        let response = client.get("http://fake.invalid/large").send().await?;
        assert!(matches!(
            response.bytes().await,
            Err(HttpClientError::BodyTooLarge)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn fake_transport_accepts_form_and_multipart_requests() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Ok(http::StatusCode::OK),
                Ok(http::StatusCode::OK),
            ])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        client
            .post("http://fake.invalid/form")
            .form(&[("kind", "test")])
            .send()
            .await?;
        client
            .post("http://fake.invalid/multipart")
            .multipart(reqwest::multipart::Form::new().text("kind", "test"))
            .send()
            .await?;
        let content_types = transport
            .content_types
            .lock()
            .map_err(|_| HttpClientError::Transport)?;
        assert_eq!(content_types.len(), 2);
        assert!(
            content_types[0]
                .as_deref()
                .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"))
        );
        assert!(
            content_types[1]
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data"))
        );
        Ok(())
    }

    struct ArcTransport(Arc<FakeTransport>);

    #[async_trait::async_trait]
    impl HttpTransport for ArcTransport {
        async fn send(
            &self,
            request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            self.0.send(request).await
        }
    }

    #[tokio::test]
    async fn explicit_retry_is_bounded_for_idempotent_requests() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Ok(http::StatusCode::SERVICE_UNAVAILABLE),
                Ok(http::StatusCode::OK),
            ])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO)?;
        let response = client
            .get("http://fake.invalid/retry")
            .idempotent()
            .send_with_retry(policy)
            .await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(transport.requests.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn explicit_retry_retries_transport_errors() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Err(HttpClientError::Transport),
                Ok(http::StatusCode::OK),
            ])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let policy = RetryPolicy::new(2, Duration::ZERO, Duration::ZERO)?;
        let response = client
            .get("http://fake.invalid/retry")
            .idempotent()
            .send_with_retry(policy)
            .await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(transport.requests.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn explicit_retry_stops_at_the_attempt_limit() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(
                std::iter::repeat_with(|| Ok(http::StatusCode::SERVICE_UNAVAILABLE))
                    .take(5)
                    .collect(),
            ),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO)?;
        let response = client
            .get("http://fake.invalid/retry")
            .idempotent()
            .send_with_retry(policy)
            .await?;
        assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(transport.requests.load(Ordering::SeqCst), 3);
        Ok(())
    }

    #[tokio::test]
    async fn one_shot_stream_body_is_not_replayed() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([Ok(http::StatusCode::OK)])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let body = reqwest::Body::wrap_stream(tokio_stream::iter([Ok::<_, std::io::Error>(
            "streamed body".to_owned(),
        )]));
        let policy = RetryPolicy::new(2, Duration::ZERO, Duration::ZERO)?;
        assert!(matches!(
            client
                .post("http://fake.invalid/stream")
                .body(body)
                .idempotent()
                .send_with_retry(policy)
                .await,
            Err(HttpClientError::UnreplayableRequest)
        ));
        let body = reqwest::Body::wrap_stream(tokio_stream::iter([Ok::<_, std::io::Error>(
            "streamed body".to_owned(),
        )]));
        client
            .post("http://fake.invalid/stream")
            .body(body)
            .send()
            .await?;
        assert_eq!(transport.requests.load(Ordering::SeqCst), 1);
        Ok(())
    }

    struct CancellationObservedTransport {
        started: Arc<std::sync::atomic::AtomicBool>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }

    struct DropObserver(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for DropObserver {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl HttpTransport for CancellationObservedTransport {
        async fn send(
            &self,
            _request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            self.started.store(true, Ordering::SeqCst);
            let _observer = DropObserver(Arc::clone(&self.dropped));
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn dropping_request_future_cancels_the_transport_future() -> Result<(), HttpClientError> {
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            CancellationObservedTransport {
                started: Arc::clone(&started),
                dropped: Arc::clone(&dropped),
            },
        )?;
        let task =
            tokio::spawn(async move { client.get("http://fake.invalid/cancel").send().await });
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        task.abort();
        assert!(task.await.is_err_and(|error| error.is_cancelled()));
        assert!(dropped.load(Ordering::SeqCst));
        Ok(())
    }

    #[test]
    fn retry_policy_rejects_unbounded_or_inconsistent_values() {
        assert!(matches!(
            RetryPolicy::new(0, Duration::ZERO, Duration::ZERO),
            Err(HttpClientError::InvalidConfig)
        ));
        assert!(matches!(
            RetryPolicy::new(6, Duration::ZERO, Duration::ZERO),
            Err(HttpClientError::InvalidConfig)
        ));
        assert!(matches!(
            RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(1)),
            Err(HttpClientError::InvalidConfig)
        ));
    }

    #[test]
    fn restricted_target_policy_rejects_unsafe_addresses_and_userinfo()
    -> Result<(), HttpClientError> {
        let public = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));
        for address in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 2)),
            IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6(
                Ipv6Addr::from_str("2001:db8::1").map_err(|_| HttpClientError::InvalidConfig)?,
            ),
            IpAddr::V6(
                Ipv6Addr::from_str("::ffff:127.0.0.1")
                    .map_err(|_| HttpClientError::InvalidConfig)?,
            ),
        ] {
            assert!(!is_public_address(address), "{address} must be rejected");
        }
        assert!(is_public_address(public));

        assert!(super::url_has_userinfo("http://user:secret@example.com/"));
        assert!(super::url_has_userinfo("http://@example.com/"));
        assert!(!super::url_has_userinfo("http://example.com/"));

        let mut internal_policy = HttpTargetPolicy::new();
        internal_policy.allow_internal(
            "internal.example",
            IpNet::from_str("10.0.0.0/8").map_err(|_| HttpClientError::InvalidConfig)?,
            [8443],
        )?;
        assert!(matches!(
            internal_policy.allow_internal(
                "internal.example",
                IpNet::from_str("192.168.0.0/16").map_err(|_| HttpClientError::InvalidConfig)?,
                [9443]
            ),
            Err(HttpClientError::InvalidConfig)
        ));
        for url in ["http://internal.example/", "http://internal.example:443/"] {
            let request = reqwest::Client::new()
                .get(url)
                .build()
                .map_err(|_| HttpClientError::InvalidConfig)?;
            assert!(matches!(
                super::validate_target(&request, &internal_policy),
                Err(HttpClientError::TargetRejected)
            ));
        }
        let request = reqwest::Client::new()
            .get("http://internal.example:8443/")
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        assert!(super::validate_target(&request, &internal_policy).is_ok());

        let request = reqwest::Client::new()
            .get("http://example.com/")
            .header(http::header::HOST, "internal.example")
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        assert!(matches!(
            super::validate_target(&request, &HttpTargetPolicy::new()),
            Err(HttpClientError::TargetRejected)
        ));
        Ok(())
    }

    struct FixedResolver {
        addresses: Vec<SocketAddr>,
        calls: Arc<AtomicUsize>,
    }

    impl reqwest::dns::Resolve for FixedResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let addresses = self.addresses.clone();
            Box::pin(async move {
                let addresses: reqwest::dns::Addrs = Box::new(addresses.into_iter());
                Ok(addresses)
            })
        }
    }

    #[tokio::test]
    async fn restricted_resolver_rejects_mixed_results_and_allows_explicit_internal_network()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut mixed_policy = HttpTargetPolicy::new();
        mixed_policy.allow_host("mixed.example")?;
        let mixed = RestrictedResolver {
            policy: Arc::new(mixed_policy),
            resolver: Arc::new(FixedResolver {
                addresses: vec!["93.184.216.34:0".parse()?, "127.0.0.1:0".parse()?],
                calls: Arc::new(AtomicUsize::new(0)),
            }),
        };
        let name = reqwest::dns::Name::from_str("mixed.example")?;
        assert!(mixed.resolve(name).await.is_err());

        let mut empty_policy = HttpTargetPolicy::new();
        empty_policy.allow_host("empty.example")?;
        let empty = RestrictedResolver {
            policy: Arc::new(empty_policy),
            resolver: Arc::new(FixedResolver {
                addresses: Vec::new(),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
        };
        let name = reqwest::dns::Name::from_str("empty.example")?;
        assert!(empty.resolve(name).await.is_err());

        let mut policy = HttpTargetPolicy::new();
        policy.allow_internal("internal.example", IpNet::from_str("127.0.0.0/8")?, [8080])?;
        let allowed = RestrictedResolver {
            policy: Arc::new(policy),
            resolver: Arc::new(FixedResolver {
                addresses: vec!["127.0.0.1:0".parse()?],
                calls: Arc::new(AtomicUsize::new(0)),
            }),
        };
        let name = reqwest::dns::Name::from_str("internal.example")?;
        let addresses = allowed
            .resolve(name)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        assert_eq!(addresses.count(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn restricted_client_connects_to_the_single_checked_dns_result()
    -> Result<(), Box<dyn std::error::Error>> {
        let app = axum::Router::new().route("/", get(|| async { "pinned" }));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let calls = Arc::new(AtomicUsize::new(0));
        let mut target_policy = HttpTargetPolicy::new();
        target_policy.allow_internal(
            "internal.example",
            IpNet::from_str("127.0.0.1/32")?,
            [address.port()],
        )?;
        let client = HttpClient::restricted_with_resolver(
            &HttpClientConfig::default(),
            target_policy,
            Arc::new(ChangingResolver {
                first: SocketAddr::new(address.ip(), 0),
                next: "10.0.0.1:0".parse()?,
                calls: Arc::clone(&calls),
            }),
        )?;
        assert!(matches!(
            client.get("http://internal.example/").send().await,
            Err(HttpClientError::TargetRejected)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let response = client
            .get(&format!("http://internal.example:{}/", address.port()))
            .send()
            .await?;
        assert_eq!(response.bytes().await?, b"pinned");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
        Ok(())
    }

    struct ChangingResolver {
        first: SocketAddr,
        next: SocketAddr,
        calls: Arc<AtomicUsize>,
    }

    impl reqwest::dns::Resolve for ChangingResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let address = if call == 0 { self.first } else { self.next };
            Box::pin(async move {
                let addresses: reqwest::dns::Addrs = Box::new(std::iter::once(address));
                Ok(addresses)
            })
        }
    }
}
