//! 面向在线连接的 WebSocket、SSE 与跨节点消息路由。

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use axum::{
    extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header::ORIGIN},
    response::{IntoResponse, Response, Sse, sse::Event},
};
#[cfg(feature = "realtime-redis")]
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, watch};
use uuid::Uuid;

use crate::{
    auth::CurrentUser,
    config::RealtimeConfig,
    core::{BoxFuture, Cancellation, Error, Result},
};

const ENVELOPE_VERSION: u8 = 1;
const REVOKE_CHANNEL: &str = "__openrust_revoke__";

/// 可供第三方实现的共享实时消息 transport。
#[async_trait]
pub trait RealtimeTransport: Send + Sync + 'static {
    /// 发布一个经过大小及过期时间校验的 JSON envelope。
    async fn publish(&self, namespace: &str, envelope: Vec<u8>) -> Result<()>;

    /// 持续接收 envelope，直到取消或 transport 故障；实现不得无限缓存消息。
    async fn run(
        &self,
        namespace: &str,
        output: mpsc::Sender<Vec<u8>>,
        cancellation: Cancellation,
    ) -> Result<()>;
}

/// 一条在线实时消息；payload 是应用提供的 UTF-8 JSON。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeMessage {
    id: String,
    channel: String,
    payload: String,
    expires_at_unix_secs: u64,
}

impl RealtimeMessage {
    /// 返回跨节点稳定的消息 ID。
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 返回应用频道名称。
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// 返回 UTF-8 JSON payload。
    pub fn payload(&self) -> &str {
        &self.payload
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    version: u8,
    node_id: String,
    user_id: i64,
    message: RealtimeMessage,
}

struct QueuedMessage {
    message: RealtimeMessage,
    _bytes: OwnedSemaphorePermit,
    _global_bytes: OwnedSemaphorePermit,
}

struct LocalConnection {
    user_id: i64,
    channel: String,
    sender: mpsc::Sender<QueuedMessage>,
    close: watch::Sender<bool>,
    queued_bytes: Arc<Semaphore>,
}

struct RealtimeInner {
    node_id: String,
    app_name: String,
    config: RealtimeConfig,
    transport: Option<Arc<dyn RealtimeTransport>>,
    connections: Mutex<HashMap<String, LocalConnection>>,
    connection_slots: Arc<Semaphore>,
    queued_bytes: Arc<Semaphore>,
    connection_count: AtomicUsize,
    accepting: AtomicBool,
}

/// 应用内在线投递服务。
#[derive(Clone)]
pub struct Realtime(Arc<RealtimeInner>);

impl std::fmt::Debug for Realtime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Realtime")
            .field("node_id", &self.0.node_id)
            .field(
                "connections",
                &self.0.connection_count.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl Realtime {
    /// 验证请求 `Origin` 必须精确匹配 `[realtime].allowed_origins`。
    ///
    /// 该检查独立于 CORS；端点应在调用 [`connect`](Self::connect) 前执行。
    pub fn validate_origin(&self, headers: &HeaderMap) -> Result<()> {
        let origin = headers
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok())
            .ok_or(Error::Forbidden("实时连接缺少有效 Origin".into()))?;
        if self
            .0
            .config
            .allowed_origins
            .iter()
            .any(|item| item == origin)
        {
            Ok(())
        } else {
            Err(Error::Forbidden("实时连接 Origin 不在允许列表中".into()))
        }
    }

    /// 为已通过应用路由认证的用户建立频道连接。
    ///
    /// 调用方必须已运行 `AuthLayer` 并使用 [`CurrentUser`] extractor；频道授权由应用在调用前完成。
    /// `authorized_until` 是认证凭据及业务授权都仍有效的最晚 Unix 时间；到期后连接自动关闭。
    pub async fn connect(
        &self,
        user: &CurrentUser,
        channel: impl Into<String>,
        authorized_until: SystemTime,
    ) -> Result<RealtimeSession> {
        let channel = channel.into();
        validate_channel(&channel)?;
        if !self.0.accepting.load(Ordering::Acquire) {
            return Err(Error::custom_with_status(
                StatusCode::SERVICE_UNAVAILABLE,
                "realtime_shutting_down",
                "实时服务正在关闭",
            ));
        }
        let expires_at_unix_secs = authorized_until
            .duration_since(UNIX_EPOCH)
            .map_err(Error::internal)?
            .as_secs();
        if expires_at_unix_secs <= unix_now()? {
            return Err(Error::Unauthorized);
        }
        let permit = Arc::clone(&self.0.connection_slots)
            .try_acquire_owned()
            .map_err(|_| {
                #[cfg(feature = "metrics")]
                metrics::counter!("openrust_realtime_connections_rejected_total").increment(1);
                Error::custom_with_status(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "realtime_connection_limit",
                    "实时连接数已达上限",
                )
            })?;
        let (sender, receiver) = mpsc::channel(self.0.config.max_queue_messages);
        let (close, close_receiver) = watch::channel(false);
        let id = Uuid::now_v7().to_string();
        let entry = LocalConnection {
            user_id: user.id(),
            channel: channel.clone(),
            sender,
            close,
            queued_bytes: Arc::new(Semaphore::new(self.0.config.max_queue_bytes)),
        };
        self.0.connections.lock().await.insert(id.clone(), entry);
        self.0.connection_count.fetch_add(1, Ordering::Relaxed);
        #[cfg(feature = "metrics")]
        metrics::gauge!("openrust_realtime_connections_in_flight").increment(1.0);
        let valid_for = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(expires_at_unix_secs))
            .and_then(|end| end.duration_since(SystemTime::now()).ok())
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_secs(self.0.config.max_connection_age_secs));
        Ok(RealtimeSession {
            realtime: self.clone(),
            id,
            receiver,
            close: close_receiver,
            _connection_slot: permit,
            expires_at: tokio::time::Instant::now() + valid_for,
            heartbeat: Duration::from_secs(self.0.config.heartbeat_interval_secs),
        })
    }

    /// 向指定用户在目标频道的所有本机及在线远端连接发送 JSON。
    ///
    /// 远端投递依赖显式配置的 transport。返回成功表示本机队列已接收且共享 transport 接受发布，
    /// 不表示远端客户端收到；任何本机队列溢出会关闭对应慢消费者并返回错误。
    pub async fn send_to_user<T: Serialize>(
        &self,
        user_id: i64,
        channel: &str,
        payload: &T,
        ttl: Duration,
    ) -> Result<usize> {
        if user_id <= 0 {
            return Err(Error::BadRequest("实时目标用户 ID 必须为正数".into()));
        }
        validate_channel(channel)?;
        if ttl < Duration::from_secs(1) || ttl > Duration::from_secs(300) {
            return Err(Error::BadRequest("实时消息 TTL 必须为 1 至 300 秒".into()));
        }
        let payload = serde_json::to_string(payload).map_err(Error::internal)?;
        if payload.len() > self.0.config.max_message_bytes {
            return Err(Error::custom_with_status(
                StatusCode::PAYLOAD_TOO_LARGE,
                "realtime_message_too_large",
                "实时消息超过配置上限",
            ));
        }
        let expires_at_unix_secs = unix_now()?.saturating_add(ttl.as_secs().max(1));
        let message = RealtimeMessage {
            id: Uuid::now_v7().to_string(),
            channel: channel.to_owned(),
            payload,
            expires_at_unix_secs,
        };
        let envelope = Envelope {
            version: ENVELOPE_VERSION,
            node_id: self.0.node_id.clone(),
            user_id,
            message: message.clone(),
        };
        let encoded = serde_json::to_vec(&envelope).map_err(Error::internal)?;
        if encoded.len() > self.0.config.max_message_bytes.saturating_add(1024) {
            return Err(Error::custom(
                "realtime_envelope_too_large",
                "实时消息 envelope 超过配置上限",
            ));
        }
        let delivered = self.deliver(user_id, &message).await?;
        if let Some(transport) = &self.0.transport {
            transport.publish(&self.topic(), encoded).await?;
        }
        #[cfg(feature = "metrics")]
        metrics::counter!("openrust_realtime_messages_sent_total").increment(1);
        Ok(delivered)
    }

    /// 关闭当前节点上该用户的所有实时连接。
    pub async fn revoke_user(&self, user_id: i64) -> Result<usize> {
        if user_id <= 0 {
            return Err(Error::BadRequest("实时撤销用户 ID 必须为正数".into()));
        }
        let local = self.revoke_local_user(user_id).await;
        if let Some(transport) = &self.0.transport {
            let envelope = Envelope {
                version: ENVELOPE_VERSION,
                node_id: self.0.node_id.clone(),
                user_id,
                message: RealtimeMessage {
                    id: Uuid::now_v7().to_string(),
                    channel: REVOKE_CHANNEL.to_owned(),
                    payload: String::new(),
                    expires_at_unix_secs: unix_now()?.saturating_add(5),
                },
            };
            transport
                .publish(
                    &self.topic(),
                    serde_json::to_vec(&envelope).map_err(Error::internal)?,
                )
                .await?;
        }
        Ok(local)
    }

    async fn revoke_local_user(&self, user_id: i64) -> usize {
        let mut connections = self.0.connections.lock().await;
        let ids = connections
            .iter()
            .filter(|(_, item)| item.user_id == user_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            if let Some(connection) = connections.remove(id) {
                connection.close.send_replace(true);
                self.0.connection_count.fetch_sub(1, Ordering::Relaxed);
            }
        }
        ids.len()
    }

    async fn deliver(&self, user_id: i64, message: &RealtimeMessage) -> Result<usize> {
        let mut connections = self.0.connections.lock().await;
        let ids = connections
            .iter()
            .filter(|(_, item)| item.user_id == user_id && item.channel == message.channel)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut delivered = 0;
        let mut slow = Vec::new();
        let matching = ids.len();
        for id in ids {
            let Some(connection) = connections.get(&id) else {
                continue;
            };
            let byte_count = u32::try_from(message.payload.len()).map_err(Error::internal)?;
            let Ok(bytes) = Arc::clone(&connection.queued_bytes).try_acquire_many_owned(byte_count)
            else {
                slow.push(id);
                continue;
            };
            let Ok(global_bytes) =
                Arc::clone(&self.0.queued_bytes).try_acquire_many_owned(byte_count)
            else {
                slow.push(id);
                continue;
            };
            match connection.sender.try_send(QueuedMessage {
                message: message.clone(),
                _bytes: bytes,
                _global_bytes: global_bytes,
            }) {
                Ok(()) => delivered += 1,
                Err(_) => slow.push(id),
            }
        }
        for id in slow {
            if let Some(connection) = connections.remove(&id) {
                connection.close.send_replace(true);
                self.0.connection_count.fetch_sub(1, Ordering::Relaxed);
            }
        }
        if delivered < matching {
            return Err(Error::custom_with_status(
                StatusCode::SERVICE_UNAVAILABLE,
                "realtime_slow_consumer",
                "实时连接队列已满，已断开慢消费者",
            ));
        }
        Ok(delivered)
    }

    async fn remove_connection(&self, id: &str) {
        if self.0.connections.lock().await.remove(id).is_some() {
            self.0.connection_count.fetch_sub(1, Ordering::Relaxed);
            #[cfg(feature = "metrics")]
            metrics::gauge!("openrust_realtime_connections_in_flight").decrement(1.0);
        }
    }

    async fn receive(&self, bytes: &[u8]) -> Result<()> {
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| {
            Error::custom("realtime_envelope_invalid", "实时 transport envelope 无效")
        })?;
        if envelope.version != ENVELOPE_VERSION
            || envelope.node_id == self.0.node_id
            || envelope.user_id <= 0
            || envelope.message.payload.len() > self.0.config.max_message_bytes
            || envelope.message.expires_at_unix_secs <= unix_now()?
        {
            return Ok(());
        }
        if envelope.message.channel == REVOKE_CHANNEL {
            self.revoke_local_user(envelope.user_id).await;
            return Ok(());
        }
        validate_channel(&envelope.message.channel)?;
        let _ = self.deliver(envelope.user_id, &envelope.message).await;
        Ok(())
    }

    fn topic(&self) -> String {
        format!(
            "openrust:realtime:v1:{}:{}",
            self.0.app_name, self.0.config.namespace
        )
    }

    async fn shutdown(&self) {
        self.0.accepting.store(false, Ordering::Release);
        let mut connections = self.0.connections.lock().await;
        #[cfg(feature = "metrics")]
        let removed = connections.len();
        for (_, connection) in connections.drain() {
            connection.close.send_replace(true);
        }
        self.0.connection_count.store(0, Ordering::Relaxed);
        #[cfg(feature = "metrics")]
        if removed > 0 {
            let removed = u32::try_from(removed).unwrap_or(u32::MAX);
            metrics::gauge!("openrust_realtime_connections_in_flight")
                .decrement(f64::from(removed));
        }
    }

    pub(crate) async fn begin_shutdown(&self) {
        self.shutdown().await;
    }
}

/// 一个已授权用户的在线会话。
pub struct RealtimeSession {
    realtime: Realtime,
    id: String,
    receiver: mpsc::Receiver<QueuedMessage>,
    close: watch::Receiver<bool>,
    _connection_slot: OwnedSemaphorePermit,
    expires_at: tokio::time::Instant,
    heartbeat: Duration,
}

impl RealtimeSession {
    /// 将会话升级为 WebSocket；客户端输入帧仅支持 close、ping 与 pong，不接受业务消息。
    pub fn websocket(self, upgrade: WebSocketUpgrade) -> Response {
        let max_message_bytes = self.realtime.0.config.max_message_bytes;
        upgrade
            .max_message_size(max_message_bytes)
            .max_frame_size(max_message_bytes)
            .on_upgrade(move |socket| self.run_websocket(socket))
            .into_response()
    }

    /// 将会话转换为 SSE；使用有限心跳，无 Last-Event-ID 或离线回放。
    pub fn sse(self) -> Response {
        let interval = self.heartbeat;
        let stream = futures_util::stream::unfold(
            (self, tokio::time::interval(interval)),
            |(mut session, mut ticker)| async move {
                tokio::select! {
                    () = tokio::time::sleep_until(session.expires_at) => {
                        session.realtime.remove_connection(&session.id).await;
                        None
                    }
                    changed = session.close.changed() => {
                        if changed.is_err() || *session.close.borrow() {
                            session.realtime.remove_connection(&session.id).await;
                            None
                        } else {
                            Some((Ok::<_, Infallible>(Event::default().comment("keep-alive")), (session, ticker)))
                        }
                    }
                    _ = ticker.tick() => Some((Ok::<_, Infallible>(Event::default().comment("keep-alive")), (session, ticker))),
                    queued = session.receiver.recv() => {
                        if let Some(queued) = queued {
                            Some((Ok::<_, Infallible>(Event::default().id(queued.message.id).data(queued.message.payload)), (session, ticker)))
                        } else {
                            session.realtime.remove_connection(&session.id).await;
                            None
                        }
                    }
                }
            },
        );
        let mut response = Sse::new(stream).into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache, no-transform"),
        );
        response.headers_mut().insert(
            axum::http::HeaderName::from_static("x-accel-buffering"),
            axum::http::HeaderValue::from_static("no"),
        );
        response
    }

    async fn run_websocket(mut self, mut socket: WebSocket) {
        let mut heartbeat = tokio::time::interval(self.heartbeat);
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(self.expires_at) => break,
                changed = self.close.changed() => {
                    if changed.is_err() || *self.close.borrow() { break; }
                }
                _ = heartbeat.tick() => {
                    if !send_until_boundary(&mut self.close, self.expires_at, socket.send(WsMessage::Ping(Vec::new().into()))).await { break; }
                }
                queued = self.receiver.recv() => {
                    let Some(queued) = queued else { break };
                    if !send_until_boundary(&mut self.close, self.expires_at, socket.send(WsMessage::Text(queued.message.payload.into()))).await { break; }
                }
                incoming = socket.recv() => {
                    match incoming {
                        Some(Ok(WsMessage::Close(_)) | Err(_)) | None => break,
                        Some(Ok(WsMessage::Ping(payload))) => {
                            if !send_until_boundary(&mut self.close, self.expires_at, socket.send(WsMessage::Pong(payload))).await { break; }
                        }
                        Some(Ok(WsMessage::Pong(_))) => {}
                        Some(Ok(WsMessage::Text(_) | WsMessage::Binary(_))) => {
                            let _ = send_until_boundary(&mut self.close, self.expires_at, socket.send(WsMessage::Close(None))).await;
                            break;
                        }
                    }
                }
            }
        }
        let _ = tokio::time::timeout(
            Duration::from_millis(100),
            socket.send(WsMessage::Close(None)),
        )
        .await;
        self.realtime.remove_connection(&self.id).await;
    }
}

async fn send_until_boundary<F, E>(
    close: &mut watch::Receiver<bool>,
    expires_at: tokio::time::Instant,
    send: F,
) -> bool
where
    F: std::future::Future<Output = std::result::Result<(), E>>,
{
    if *close.borrow() || tokio::time::Instant::now() >= expires_at {
        return false;
    }
    tokio::select! {
        biased;
        () = tokio::time::sleep_until(expires_at) => false,
        changed = close.changed() => {
            let _ = changed;
            false
        }
        result = tokio::time::timeout(Duration::from_secs(5), send) => matches!(result, Ok(Ok(()))),
    }
}

impl Drop for RealtimeSession {
    fn drop(&mut self) {
        let realtime = self.realtime.clone();
        let id = self.id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move { realtime.remove_connection(&id).await });
        }
    }
}

/// 使用统一容器管理实时连接及共享 transport 生命周期。
pub struct RealtimeServiceProvider {
    config: Option<RealtimeConfig>,
    transport: Option<Arc<dyn RealtimeTransport>>,
}

impl RealtimeServiceProvider {
    /// 使用当前配置仓储。
    pub fn new() -> Self {
        Self {
            config: None,
            transport: None,
        }
    }
    /// 使用显式实时配置。
    pub fn with_config(config: RealtimeConfig) -> Self {
        Self {
            config: Some(config),
            transport: None,
        }
    }
    /// 使用应用提供的共享 transport。
    #[must_use]
    pub fn with_transport(mut self, transport: impl RealtimeTransport) -> Self {
        self.transport = Some(Arc::new(transport));
        self
    }
}
impl Default for RealtimeServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}
impl crate::core::ServiceProvider for RealtimeServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema as S;
            app.config_section(
                "realtime",
                RealtimeConfig::default(),
                S::object([
                    ("max_connections", S::Integer),
                    ("max_queue_messages", S::Integer),
                    ("max_queue_bytes", S::Integer),
                    ("max_total_queue_bytes", S::Integer),
                    ("max_message_bytes", S::Integer),
                    ("heartbeat_interval_secs", S::Integer),
                    ("max_connection_age_secs", S::Integer),
                    ("namespace", S::String),
                    ("allowed_origins", S::array(S::String)),
                    ("redis_url", S::String.nullable().sensitive()),
                ]),
                |config: &RealtimeConfig| {
                    config
                        .validate()
                        .map_err(|error| Error::custom("invalid_realtime_config", error))
                },
            )?;
            let explicit = self.config.clone();
            let supplied = self.transport.clone();
            app.singleton::<Realtime, _>(move |resolver| {
                let explicit = explicit.clone();
                let supplied = supplied.clone();
                Box::pin(async move {
                    let config = match explicit {
                        Some(config) => config,
                        None => resolver.section::<RealtimeConfig>("realtime")?,
                    };
                    config
                        .validate()
                        .map_err(|error| Error::custom("invalid_realtime_config", error))?;
                    let app_config = resolver.section::<crate::config::AppConfig>("app")?;
                    #[cfg(feature = "realtime-redis")]
                    let transport = match (supplied, config.redis_url()) {
                        (Some(transport), _) => Some(transport),
                        (None, Some(url)) => Some(Arc::new(RedisRealtimeTransport::connect(url)?)
                            as Arc<dyn RealtimeTransport>),
                        (None, None) => None,
                    };
                    #[cfg(not(feature = "realtime-redis"))]
                    let transport = supplied;
                    if config.redis_url().is_some() && transport.is_none() {
                        return Err(Error::custom(
                            "realtime_transport_unavailable",
                            "Redis 实时驱动不可用或 realtime-redis feature 未启用",
                        ));
                    }
                    Ok(Arc::new(Realtime(Arc::new(RealtimeInner {
                        node_id: Uuid::now_v7().to_string(),
                        app_name: app_config.name,
                        connection_slots: Arc::new(Semaphore::new(config.max_connections)),
                        queued_bytes: Arc::new(Semaphore::new(config.max_total_queue_bytes)),
                        config,
                        transport,
                        connections: Mutex::new(HashMap::new()),
                        connection_count: AtomicUsize::new(0),
                        accepting: AtomicBool::new(true),
                    }))))
                })
            })
            .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.config_repository();
            let has_custom_transport = self.transport.is_some();
            app.validate_at_finalize(move || {
                Box::pin(async move {
                    let config = config.section::<RealtimeConfig>("realtime")?;
                    if let Some(url) = config.redis_url() {
                        url::Url::parse(url).map_err(|_| {
                            Error::custom("realtime_redis_config", "Redis 实时 URL 无效")
                        })?;
                        #[cfg(feature = "realtime-redis")]
                        {
                            redis::Client::open(url).map_err(|_| {
                                Error::custom("realtime_redis_config", "Redis 实时 URL 无效")
                            })?;
                        }
                        #[cfg(not(feature = "realtime-redis"))]
                        if !has_custom_transport {
                            return Err(Error::custom(
                                "realtime_transport_unavailable",
                                "Redis 实时驱动不可用或 realtime-redis feature 未启用",
                            ));
                        }
                        #[cfg(feature = "realtime-redis")]
                        let _ = has_custom_transport;
                    }
                    Ok(())
                })
            });
            let service = app.resolve::<Realtime>().await?;
            let transport = service.0.transport.clone();
            let task_service = service.clone();
            let namespace = service.topic();
            app.resources().spawn(move |cancellation| async move {
                let (sender, mut receiver) = mpsc::channel::<Vec<u8>>(32);
                let mut backoff = Duration::from_millis(100);
                loop {
                    if cancellation.is_cancelled() { break; }
                    if let Some(transport) = &transport {
                        let mut run = Box::pin(transport.run(&namespace, sender.clone(), Cancellation::child(&cancellation)));
                        let mut retry = false;
                        loop {
                            tokio::select! {
                                () = cancellation.cancelled() => break,
                                result = &mut run => {
                                    if let Err(error) = result {
                                        tracing::error!(code = error.code(), "realtime transport listener failed; retrying");
                                    }
                                    retry = true;
                                    break;
                                }
                                message = receiver.recv() => receive_message(&task_service, message).await,
                            }
                        }
                        if cancellation.is_cancelled() { break; }
                        if retry {
                            tokio::select! { () = cancellation.cancelled() => break, () = tokio::time::sleep(backoff) => {} }
                            backoff = (backoff * 2).min(Duration::from_secs(5));
                        }
                    } else {
                        cancellation.cancelled().await;
                        break;
                    }
                }
                task_service.shutdown().await;
            })?;
            Ok(())
        })
    }
}

async fn receive_message(service: &Realtime, message: Option<Vec<u8>>) {
    if let Some(message) = message
        && let Err(error) = service.receive(&message).await
    {
        tracing::warn!(
            code = error.code(),
            "invalid realtime transport envelope dropped"
        );
    }
}

fn validate_channel(channel: &str) -> Result<()> {
    if channel.is_empty()
        || channel.len() > 128
        || channel == REVOKE_CHANNEL
        || !channel
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    {
        return Err(Error::BadRequest(
            "实时频道必须是 1..=128 位安全 ASCII 名称".into(),
        ));
    }
    Ok(())
}

fn unix_now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(Error::internal)
}

#[cfg(feature = "realtime-redis")]
struct RedisRealtimeTransport {
    client: redis::Client,
    connection: tokio::sync::OnceCell<redis::aio::ConnectionManager>,
}

#[cfg(feature = "realtime-redis")]
impl RedisRealtimeTransport {
    fn connect(url: &str) -> Result<Self> {
        let client = redis::Client::open(url)
            .map_err(|_| Error::custom("realtime_redis_config", "Redis 实时 URL 无效"))?;
        Ok(Self {
            client,
            connection: tokio::sync::OnceCell::new(),
        })
    }
}

#[cfg(feature = "realtime-redis")]
#[async_trait]
impl RealtimeTransport for RedisRealtimeTransport {
    async fn publish(&self, namespace: &str, envelope: Vec<u8>) -> Result<()> {
        use redis::AsyncCommands;
        let manager = self
            .connection
            .get_or_try_init(|| async {
                redis::aio::ConnectionManager::new(self.client.clone())
                    .await
                    .map_err(|_| {
                        Error::custom_with_status(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "realtime_transport_unavailable",
                            "实时共享 transport 暂不可用",
                        )
                    })
            })
            .await?;
        let mut connection = manager.clone();
        let _: usize = connection.publish(namespace, envelope).await.map_err(|_| {
            Error::custom_with_status(
                StatusCode::SERVICE_UNAVAILABLE,
                "realtime_transport_unavailable",
                "实时共享 transport 暂不可用",
            )
        })?;
        Ok(())
    }

    async fn run(
        &self,
        namespace: &str,
        output: mpsc::Sender<Vec<u8>>,
        cancellation: Cancellation,
    ) -> Result<()> {
        let mut pubsub = self
            .client
            .get_async_pubsub()
            .await
            .map_err(Error::internal)?;
        pubsub.subscribe(namespace).await.map_err(Error::internal)?;
        let mut messages = pubsub.on_message();
        loop {
            tokio::select! {
                () = cancellation.cancelled() => return Ok(()),
                message = messages.next() => {
                    let Some(message) = message else { return Err(Error::custom("realtime_redis_closed", "Redis 实时订阅已关闭")); };
                    let payload: Vec<u8> = message.get_payload().map_err(Error::internal)?;
                    if output.send(payload).await.is_err() { return Ok(()); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Realtime, RealtimeConfig, RealtimeInner, send_until_boundary};
    use crate::auth::CurrentUser;
    use crate::core::{Error, Result};
    use std::{
        collections::HashMap,
        sync::atomic::AtomicUsize,
        time::{Duration, SystemTime},
    };
    use tokio::sync::{Mutex, Semaphore, watch};

    fn service(config: RealtimeConfig) -> Realtime {
        Realtime(std::sync::Arc::new(RealtimeInner {
            node_id: "node-test".into(),
            app_name: "test-app".into(),
            connection_slots: std::sync::Arc::new(Semaphore::new(config.max_connections)),
            queued_bytes: std::sync::Arc::new(Semaphore::new(config.max_total_queue_bytes)),
            config,
            transport: None,
            connections: Mutex::new(HashMap::new()),
            connection_count: AtomicUsize::new(0),
            accepting: std::sync::atomic::AtomicBool::new(true),
        }))
    }

    fn authorized_until() -> SystemTime {
        SystemTime::now() + Duration::from_secs(30)
    }

    #[tokio::test]
    async fn stalled_websocket_send_yields_to_shutdown_and_jwt_expiry() -> Result<()> {
        let (sender, mut receiver) = watch::channel(false);
        let stopped = tokio::spawn(async move {
            send_until_boundary(
                &mut receiver,
                tokio::time::Instant::now() + Duration::from_secs(10),
                std::future::pending::<std::result::Result<(), ()>>(),
            )
            .await
        });
        tokio::task::yield_now().await;
        sender.send_replace(true);
        let stopped = tokio::time::timeout(Duration::from_secs(1), stopped)
            .await
            .map_err(Error::internal)?
            .map_err(Error::internal)?;
        assert!(!stopped);

        let (_, mut receiver) = watch::channel(false);
        assert!(
            !send_until_boundary(
                &mut receiver,
                tokio::time::Instant::now() + Duration::from_millis(10),
                std::future::pending::<std::result::Result<(), ()>>(),
            )
            .await
        );

        let (sender, mut receiver) = watch::channel(false);
        let _keep_sender = sender;
        let started = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(6),
            send_until_boundary(
                &mut receiver,
                started + Duration::from_secs(10),
                std::future::pending::<std::result::Result<(), ()>>(),
            ),
        )
        .await
        .map_err(Error::internal)?;
        assert!(!result);
        assert!(started.elapsed() >= Duration::from_secs(5));
        Ok(())
    }

    #[tokio::test]
    async fn user_channel_delivery_and_revocation_close_online_session() -> crate::core::Result<()>
    {
        let realtime = service(RealtimeConfig::new());
        let user = CurrentUser::test_user(73);
        let mut session = realtime
            .connect(&user, "notifications", authorized_until())
            .await?;
        let count = realtime
            .send_to_user(
                73,
                "notifications",
                &serde_json::json!({"ok": true}),
                Duration::from_secs(5),
            )
            .await?;
        assert_eq!(count, 1);
        assert!(
            session
                .receiver
                .recv()
                .await
                .is_some_and(|queued| queued.message.channel == "notifications")
        );
        assert_eq!(realtime.revoke_user(73).await?, 1);
        assert!(*session.close.borrow());
        Ok(())
    }

    #[tokio::test]
    async fn slow_consumer_is_closed_at_message_queue_bound() -> crate::core::Result<()> {
        let mut config = RealtimeConfig::new();
        config.max_queue_messages = 1;
        let realtime = service(config);
        let user = CurrentUser::test_user(73);
        let session = realtime
            .connect(&user, "updates", authorized_until())
            .await?;
        realtime
            .send_to_user(73, "updates", &"first", Duration::from_secs(5))
            .await?;
        let error = realtime
            .send_to_user(73, "updates", &"second", Duration::from_secs(5))
            .await;
        assert!(error.is_err());
        assert_eq!(
            realtime
                .0
                .connection_count
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        drop(session);
        Ok(())
    }

    #[tokio::test]
    async fn connection_expiry_origin_and_payload_limits_fail_closed() -> crate::core::Result<()> {
        let mut config = RealtimeConfig::new();
        config.allowed_origins = vec!["https://app.example".into()];
        config.max_message_bytes = 8;
        let realtime = service(config);
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::ORIGIN,
            "https://app.example"
                .parse()
                .map_err(crate::core::Error::internal)?,
        );
        assert!(realtime.validate_origin(&headers).is_ok());
        headers.insert(
            http::header::ORIGIN,
            "https://evil.example"
                .parse()
                .map_err(crate::core::Error::internal)?,
        );
        assert!(realtime.validate_origin(&headers).is_err());
        let user = CurrentUser::test_user(73);
        assert!(
            realtime
                .connect(&user, "updates", SystemTime::now())
                .await
                .is_err()
        );
        assert!(
            realtime
                .send_to_user(73, "updates", &"payload too long", Duration::from_secs(5))
                .await
                .is_err()
        );
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
                async move { app.overlay("realtime", serde_json::json!({"max_connections": 0})) },
            )
        }
    }
    #[tokio::test]
    async fn singleton_and_shutdown_use_managed_task() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(RealtimeServiceProvider::new());
        let runtime = builder
            .start(Figment::from(Serialized::defaults(
                crate::config::FrameworkConfig::default(),
            )))
            .await?;
        let service = runtime.container().resolve::<Realtime>().await?;
        assert!(Arc::ptr_eq(
            &service,
            &runtime.container().resolve::<Realtime>().await?
        ));
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        assert!(!service.0.accepting.load(Ordering::SeqCst));
        Ok(())
    }
    struct RedisOverlay(&'static str);
    impl ServiceProvider for RedisOverlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(
                async move { app.overlay("realtime", serde_json::json!({"redis_url": self.0})) },
            )
        }
    }
    #[tokio::test]
    async fn invalid_late_redis_url_fails_without_exposing_value() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(RealtimeServiceProvider::new())
            .provider(RedisOverlay("redis://[secret-sentinel"));
        let error = builder
            .start(Figment::from(Serialized::defaults(
                crate::config::FrameworkConfig::default(),
            )))
            .await
            .err()
            .ok_or_else(|| Error::custom("test_expected_error", "invalid Redis URL accepted"))?;
        assert!(!error.to_string().contains("secret-sentinel"));
        Ok(())
    }
    #[cfg(not(feature = "realtime-redis"))]
    #[tokio::test]
    async fn late_redis_url_without_driver_fails() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(RealtimeServiceProvider::new())
            .provider(RedisOverlay("redis://localhost/"));
        assert!(
            builder
                .start(Figment::from(Serialized::defaults(
                    crate::config::FrameworkConfig::default()
                )))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn late_invalid_overlay_fails() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(RealtimeServiceProvider::new())
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
