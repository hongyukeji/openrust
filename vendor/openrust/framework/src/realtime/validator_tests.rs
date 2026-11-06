//! 核心真实输出边界与回调取消回归；不依赖数据库或共享服务。
use super::*;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use std::sync::Mutex as SyncMutex;
use tokio::sync::Notify;

struct Validator {
    allowed: AtomicBool,
    pending: AtomicBool,
    deadline: SyncMutex<SystemTime>,
    entered: Notify,
    calls: AtomicUsize,
}
impl Validator {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            allowed: AtomicBool::new(true),
            pending: AtomicBool::new(false),
            deadline: SyncMutex::new(SystemTime::now() + Duration::from_secs(30)),
            entered: Notify::new(),
            calls: AtomicUsize::new(0),
        })
    }
    fn until(&self, deadline: SystemTime) -> Result<()> {
        *self.deadline.lock().map_err(|_| Error::Unauthorized)? = deadline;
        Ok(())
    }
}
#[async_trait]
impl RealtimeValidator for Validator {
    async fn validate(&self) -> Result<SystemTime> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.pending.load(Ordering::SeqCst) {
            self.entered.notify_one();
            return std::future::pending().await;
        }
        if !self.allowed.load(Ordering::SeqCst) {
            self.entered.notify_one();
            return Err(Error::Unauthorized);
        }
        Ok(*self.deadline.lock().map_err(|_| Error::Unauthorized)?)
    }
}
fn service() -> Realtime {
    let mut config = RealtimeConfig::new();
    config.heartbeat_interval_secs = 1;
    Realtime(Arc::new(RealtimeInner {
        node_id: "validator-node".into(),
        app_name: "validator-test".into(),
        connection_slots: Arc::new(Semaphore::new(config.max_connections)),
        queued_bytes: Arc::new(Semaphore::new(config.max_total_queue_bytes)),
        config,
        transport: None,
        connections: Mutex::new(HashMap::new()),
        connection_count: AtomicUsize::new(0),
        accepting: AtomicBool::new(true),
    }))
}
async fn connect(realtime: &Realtime, validator: Arc<Validator>) -> Result<RealtimeSession> {
    realtime
        .connect_with_validator(
            &CurrentUser::test_user(73),
            "owned",
            SystemTime::now() + Duration::from_secs(20),
            validator,
        )
        .await
}
fn check(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::custom(
            "realtime_validator_contract",
            "实时凭据校验契约断言失败",
        ))
    }
}
async fn frame(body: &mut axum::body::Body) -> Result<Option<http_body::Frame<axum::body::Bytes>>> {
    tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .map_err(Error::internal)?
        .transpose()
        .map_err(Error::internal)
}

#[tokio::test]
async fn validator_rejects_before_registering_and_preserves_fixed_deadline() -> Result<()> {
    let realtime = service();
    let validator = Validator::new();
    validator.allowed.store(false, Ordering::SeqCst);
    check(connect(&realtime, validator.clone()).await.is_err())?;
    check(realtime.0.connections.lock().await.is_empty())?;
    check(realtime.0.connection_count.load(Ordering::SeqCst) == 0)?;
    validator.allowed.store(true, Ordering::SeqCst);
    let mut session = connect(&realtime, validator.clone()).await?;
    let original = session.expires_at;
    validator.until(SystemTime::now() + Duration::from_secs(100))?;
    check(session.authorize_io().await && session.expires_at <= original)?;
    validator.until(SystemTime::now() + Duration::from_millis(150))?;
    check(session.authorize_io().await && session.expires_at < original)?;
    let shortened = session.expires_at;
    validator.until(SystemTime::now() + Duration::from_secs(100))?;
    check(session.authorize_io().await && session.expires_at <= shortened)?;
    validator.pending.store(true, Ordering::SeqCst);
    check(
        !tokio::time::timeout(Duration::from_millis(500), session.authorize_io())
            .await
            .map_err(Error::internal)?,
    )?;
    Ok(())
}

#[tokio::test]
async fn sse_revocation_suppresses_queued_business_and_heartbeat() -> Result<()> {
    for queue_business in [true, false] {
        let realtime = service();
        let validator = Validator::new();
        let session = connect(&realtime, validator.clone()).await?;
        let id = session.id.clone();
        let mut body = session.sse().into_body();
        check(frame(&mut body).await?.is_some())?; // 首次心跳真实经过validator。
        check(validator.calls.load(Ordering::SeqCst) >= 2)?;
        validator.allowed.store(false, Ordering::SeqCst);
        if queue_business {
            realtime
                .send_to_user(73, "owned", &"must-not-leak", Duration::from_secs(5))
                .await?;
        }
        check(frame(&mut body).await?.is_none())?;
        check(!realtime.0.connections.lock().await.contains_key(&id))?;
        check(realtime.0.connection_count.load(Ordering::SeqCst) == 0)?;
    }
    Ok(())
}

#[tokio::test]
async fn pending_validator_does_not_hold_connection_registry_and_close_cancels_it() -> Result<()> {
    let realtime = service();
    let validator = Validator::new();
    let mut session = connect(&realtime, validator.clone()).await?;
    validator.pending.store(true, Ordering::SeqCst);
    let task = tokio::spawn(async move {
        let accepted = session.authorize_io().await;
        drop(session);
        accepted
    });
    tokio::time::timeout(Duration::from_secs(1), validator.entered.notified())
        .await
        .map_err(Error::internal)?;
    let other = tokio::time::timeout(
        Duration::from_millis(300),
        realtime.connect(
            &CurrentUser::test_user(74),
            "other",
            SystemTime::now() + Duration::from_secs(10),
        ),
    )
    .await
    .map_err(Error::internal)??;
    let revoked = tokio::time::timeout(Duration::from_millis(300), realtime.revoke_user(73))
        .await
        .map_err(Error::internal)??;
    check(revoked == 1)?;
    check(
        !tokio::time::timeout(Duration::from_millis(300), task)
            .await
            .map_err(Error::internal)?
            .map_err(Error::internal)?,
    )?;
    drop(other);
    Ok(())
}

#[tokio::test]
async fn pending_validator_times_out_and_sse_unregisters() -> Result<()> {
    let realtime = service();
    let validator = Validator::new();
    let session = connect(&realtime, validator.clone()).await?;
    let id = session.id.clone();
    validator.pending.store(true, Ordering::SeqCst);
    let mut body = session.sse().into_body();
    check(frame(&mut body).await?.is_none())?;
    check(!realtime.0.connections.lock().await.contains_key(&id))?;
    check(realtime.0.connection_count.load(Ordering::SeqCst) == 0)?;
    Ok(())
}

#[tokio::test]
async fn websocket_revocation_suppresses_business_and_heartbeat() -> Result<()> {
    for queue_business in [true, false] {
        let realtime = service();
        let validator = Validator::new();
        let route_realtime = realtime.clone();
        let route_validator = validator.clone();
        let router =
            axum::Router::new().route(
                "/socket",
                axum::routing::get(move |upgrade: WebSocketUpgrade| {
                    let realtime = route_realtime.clone();
                    let validator = route_validator.clone();
                    async move {
                        Ok::<_, Error>(connect(&realtime, validator).await?.websocket(upgrade))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(Error::internal)?;
        let address = listener.local_addr().map_err(Error::internal)?;
        let server = tokio::spawn(async move { axum::serve(listener, router).await });
        let outcome: Result<()> = async {
            let (mut socket, _) =
                tokio_tungstenite::connect_async(format!("ws://{address}/socket"))
                    .await
                    .map_err(Error::internal)?;
            let first = tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .map_err(Error::internal)?
                .ok_or(Error::Unauthorized)?
                .map_err(Error::internal)?;
            check(matches!(
                first,
                tokio_tungstenite::tungstenite::Message::Ping(_)
            ))?;
            validator.allowed.store(false, Ordering::SeqCst);
            if queue_business {
                realtime
                    .send_to_user(73, "owned", &"must-not-leak", Duration::from_secs(5))
                    .await?;
            }
            // 不再poll客户端，避免自动Pong先触发入帧校验；等待服务端业务/心跳回调真正拒绝。
            tokio::time::timeout(Duration::from_secs(2), validator.entered.notified())
                .await
                .map_err(Error::internal)?;
            let next = tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .map_err(Error::internal)?;
            match next {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_)) | None => {}
                _ => return Err(Error::Unauthorized),
            }
            // 收到Close可能早于服务端remove；有限等待注销而不依赖sleep。
            tokio::time::timeout(Duration::from_secs(1), async {
                while !realtime.0.connections.lock().await.is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(Error::internal)?;
            check(realtime.0.connection_count.load(Ordering::SeqCst) == 0)
        }
        .await;
        server.abort();
        let _ = server.await;
        outcome?;
    }
    Ok(())
}

#[tokio::test]
async fn pending_handshake_timeout_never_registers_connection() -> Result<()> {
    let realtime = service();
    let validator = Validator::new();
    validator.pending.store(true, Ordering::SeqCst);
    let result = tokio::time::timeout(Duration::from_millis(1500), connect(&realtime, validator))
        .await
        .map_err(Error::internal)?;
    check(result.is_err())?;
    check(realtime.0.connections.lock().await.is_empty())?;
    check(realtime.0.connection_count.load(Ordering::SeqCst) == 0)
}
