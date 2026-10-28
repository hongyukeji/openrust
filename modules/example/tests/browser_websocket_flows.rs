//! Cookie 握手的 TCP 精确状态回归与 Chromium 原生运行时验收分别提供证据。
//! 浏览器负例只要求原生 error/close，不声称 WebSocket API 暴露 HTTP 状态。
#![cfg(all(
    feature = "auth",
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]
use super::models::article;
use figment::{Figment, providers::Serialized};
use futures_util::{SinkExt, StreamExt};
use openrust::Db;
use openrust::auth::{AuthFlows, JwtTokens};
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum;
use openrust::testing::{TestApp, TestDatabase};
use std::{
    net::SocketAddr,
    time::{Duration, SystemTime},
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Error as WsError, Message, client::IntoClientRequest},
};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult<T = ()> = std::result::Result<T, TestError>;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const ORIGIN: &str = "http://example.test";
const PASSWORD: &str = "correct horse battery staple";
const COOKIE: &str = "example_browser_session";
const BUDGET: Duration = Duration::from_secs(8);
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct ListenerTask(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for ListenerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    server: ListenerTask,
    id: i64,
    owner: String,
    other: String,
    token: String,
    origin: String,
    owner_email: String,
    other_email: String,
    password: String,
    _storage: tempfile::TempDir,
}
impl Fixture {
    async fn create(ttl: u64) -> TestResult<Self> {
        Self::create_with_listener(ttl, None).await
    }
    #[allow(clippy::too_many_lines)] // 单一 fixture 保持同一注册、真实账户和监听生命周期。
    async fn create_with_listener(
        ttl: u64,
        native_listener: Option<TcpListener>,
    ) -> TestResult<Self> {
        let native = native_listener.is_some();
        let listener = match native_listener {
            Some(listener) => listener,
            None => TcpListener::bind("127.0.0.1:0").await?,
        };
        let address = listener.local_addr()?;
        let origin = if native {
            format!("http://{address}")
        } else {
            ORIGIN.into()
        };
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let owner_email = format!("browser-owner-{suffix}@example.test");
        let other_email = format!("browser-other-{suffix}@example.test");
        let password = if native {
            format!("browser-fixture-{suffix}")
        } else {
            PASSWORD.into()
        };
        let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let storage = tempfile::tempdir()?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let sources=Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test","url":origin},"database":{"url":database.url()},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage.path()}}},
            "auth":{"jwt":{"issuer":origin,"audience":"browser-socket","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":origin,"active_key_id":"test","keys":{"test":key}}},
            "session":{"cookie_name":COOKIE,"secure":false,"http_only":true,"same_site":"strict","ttl_secs":ttl},
            "realtime":{"max_connections":1,"max_message_bytes":256,"heartbeat_interval_secs":1,"allowed_origins":[origin]}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(sources)
            .build()
            .await?;
        let flows = app.app()?.state().resolve::<AuthFlows>().await?;
        let mut owner_id = None;
        for (index, email) in [owner_email.as_str(), other_email.as_str()]
            .into_iter()
            .enumerate()
        {
            let source = format!("browser-fixture-{index}");
            let verification = flows
                .register(email, &password, &source)
                .await?
                .ok_or("verification missing")?;
            assert!(
                flows
                    .verify_email(email, verification.code(), &source)
                    .await?
            );
            let credentials = flows.login(email, &password, &source).await?;
            if index == 0 {
                owner_id = Some(credentials.user_id());
            }
        }
        let owner_id = owner_id.ok_or("owner not registered")?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let article = article::ActiveModel {
            owner_id: Set(owner_id),
            title: Set("浏览器实时文章".into()),
            body: Set("Cookie 身份授权".into()),
            ..Default::default()
        }
        .insert(&db.primary_connection()?)
        .await?;
        let token = app
            .app()?
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(owner_id)?;
        let router = openrust::http::build_router(app.app()?)?;
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        });
        let mut fixture = Self {
            app,
            database,
            address,
            stop: Some(stop),
            server: ListenerTask(server),
            id: article.id,
            owner: String::new(),
            other: String::new(),
            token,
            origin,
            owner_email,
            other_email,
            password,
            _storage: storage,
        };
        if !native {
            fixture.other = fixture.login(&fixture.other_email).await?;
            // 短 TTL 用例最后登录 owner，避免准备其他身份耗尽有效期。
            fixture.owner = fixture.login(&fixture.owner_email).await?;
        }
        Ok(fixture)
    }
    async fn login(&self, email: &str) -> TestResult<String> {
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(BUDGET)
            .build()?
            .post(format!(
                "http://{}/api/example/account/session/login",
                self.address
            ))
            .header("origin", &self.origin)
            .header("x-example-request", "same-origin")
            .json(&serde_json::json!({"email":email,"password":self.password}))
            .send()
            .await?;
        assert_eq!(
            response.status().as_u16(),
            200,
            "Session login must succeed"
        );
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|value| {
                let pair = value.split(';').next()?;
                pair.starts_with(&format!("{COOKIE}="))
                    .then(|| pair.to_owned())
            })
            .ok_or_else(|| "Session cookie missing".into())
    }
    fn request(
        &self,
        cookie: Option<&str>,
        origin: Option<&str>,
        query: &str,
    ) -> TestResult<axum::http::Request<()>> {
        let mut request = format!(
            "ws://{}/api/example/articles/{}/browser-socket{query}",
            self.address, self.id
        )
        .into_client_request()?;
        if let Some(cookie) = cookie {
            request.headers_mut().insert("cookie", cookie.parse()?);
        }
        if let Some(origin) = origin {
            request.headers_mut().insert("origin", origin.parse()?);
        }
        Ok(request)
    }
    async fn connect(&self) -> TestResult<Socket> {
        Ok(tokio::time::timeout(
            BUDGET,
            connect_async(self.request(Some(&self.owner), Some(ORIGIN), "")?),
        )
        .await??
        .0)
    }
    async fn reopen(&self) -> TestResult<Socket> {
        tokio::time::timeout(BUDGET, async {
            loop {
                match connect_async(self.request(Some(&self.owner), Some(ORIGIN), "")?).await {
                    Ok((socket, _)) => return Ok(socket),
                    Err(WsError::Http(response)) if response.status().as_u16() == 503 => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(_) => return Err("unexpected reconnect failure".into()),
                }
            }
        })
        .await?
    }
    async fn close(mut self) -> TestResult {
        let app = self
            .app
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await;
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let server = tokio::time::timeout(BUDGET, &mut self.server.0).await;
        app?;
        server???;
        self.database.close().await?;
        Ok(())
    }
}
async fn rejected(request: axum::http::Request<()>, status: u16) -> TestResult {
    match tokio::time::timeout(BUDGET, connect_async(request)).await? {
        Err(WsError::Http(response)) => {
            assert_eq!(response.status().as_u16(), status);
            Ok(())
        }
        // 不打印请求/响应，避免错误上下文泄露 Cookie 或 Authorization。
        _ => Err("unexpected WebSocket handshake result".into()),
    }
}
async fn closed(socket: &mut Socket) -> TestResult {
    tokio::time::timeout(BUDGET, async {
        loop {
            match socket.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => return Ok(()),
                Some(Ok(Message::Ping(bytes))) => {
                    if socket.send(Message::Pong(bytes)).await.is_err() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    })
    .await?
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL；真实 TCP 协议不代替浏览器 runtime 验收"]
async fn browser_session_handshake_enforces_origin_owner_credentials_and_slot_limits() -> TestResult
{
    let f = Fixture::create(120).await?;
    let outcome = async {
        rejected(f.request(None, Some(ORIGIN), "")?, 401).await?;
        rejected(f.request(Some(&f.other), Some(ORIGIN), "")?, 404).await?;
        for origin in [None, Some("null"), Some("http://evil.example.test")] {
            rejected(f.request(Some(&f.owner), origin, "")?, 403).await?;
        }
        let mut duplicate_origin = f.request(Some(&f.owner), Some(ORIGIN), "")?;
        duplicate_origin
            .headers_mut()
            .append("origin", ORIGIN.parse()?);
        rejected(duplicate_origin, 403).await?;
        let duplicate_cookie = format!("{}; {}", f.owner, f.owner);
        rejected(f.request(Some(&duplicate_cookie), Some(ORIGIN), "")?, 400).await?;
        let mut mixed = f.request(Some(&f.owner), Some(ORIGIN), "")?;
        mixed
            .headers_mut()
            .insert("authorization", format!("Bearer {}", f.token).parse()?);
        rejected(mixed, 401).await?;
        rejected(
            f.request(Some(&f.owner), Some(ORIGIN), "?access_token=forbidden")?,
            401,
        )
        .await?;
        let mut socket = f.connect().await?;
        rejected(f.request(Some(&f.owner), Some(ORIGIN), "")?, 503).await?;
        // 当前频道只读；客户端数据帧不能成为业务发布入口。
        socket
            .send(Message::Text("unauthorized client publication".into()))
            .await?;
        closed(&mut socket).await?;
        let mut second = f.reopen().await?;
        second.close(None).await?;
        closed(&mut second).await?;
        // 原生客户端仍使用原 Bearer 路由。
        let mut native = format!("ws://{}/api/example/articles/{}/socket", f.address, f.id)
            .into_client_request()?;
        native
            .headers_mut()
            .insert("authorization", format!("Bearer {}", f.token).parse()?);
        native.headers_mut().insert("origin", ORIGIN.parse()?);
        let mut native = tokio::time::timeout(BUDGET, async {
            loop {
                match connect_async(native.clone()).await {
                    Ok((socket, _)) => return Ok::<_, TestError>(socket),
                    Err(WsError::Http(response)) if response.status().as_u16() == 503 => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(_) => return Err("native Bearer regression".into()),
                }
            }
        })
        .await??;
        native.close(None).await?;
        closed(&mut native).await?;
        Ok::<(), TestError>(())
    }
    .await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL；验证持久 Session 截止，不使用滑动 expiry_date"]
async fn browser_socket_closes_at_persisted_session_expiry() -> TestResult {
    let f = Fixture::create(5).await?;
    let outcome = async {
        let db = f.app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let table = db.quoted_table_name("sessions")?;
        // 只读取 owner 最后登录的截止，不读取或输出 Session ID/hash/data。
        let row = connection
            .query_one_raw(Statement::from_string(
                connection.get_database_backend(),
                format!("SELECT expires_at FROM {table} ORDER BY expires_at DESC LIMIT 1"),
            ))
            .await?
            .ok_or("persisted Session missing")?;
        let expiry: chrono::DateTime<chrono::Utc> = row.try_get("", "expires_at")?;
        let deadline: SystemTime = expiry.into();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut socket = f.connect().await?;
        let remaining = deadline.duration_since(SystemTime::now())? + Duration::from_millis(500);
        tokio::time::timeout(remaining, closed(&mut socket)).await??;
        if let Ok(remaining) = deadline.duration_since(SystemTime::now()) {
            tokio::time::sleep(remaining + Duration::from_millis(20)).await;
        }
        rejected(f.request(Some(&f.owner), Some(ORIGIN), "")?, 401).await?;
        Ok::<(), TestError>(())
    }
    .await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL；真实 TCP 活连接停机回收"]
async fn browser_socket_is_closed_by_application_shutdown() -> TestResult {
    let f = Fixture::create(120).await?;
    let mut socket = f.connect().await?;
    let (shutdown, disconnected) = tokio::join!(f.close(), closed(&mut socket));
    shutdown?;
    disconnected
}

/// 清理前保留未 reap 的 leader PID，避免已退出父进程的 PGID 被无关进程复用。
/// 仅覆盖本进程组，不声称可回收自行 setsid 逃逸的子孙。
struct NativeChild {
    child: Option<tokio::process::Child>,
    pgid: u32,
}
async fn kill_native_group(id: u32) -> TestResult {
    #[cfg(unix)]
    {
        let output = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{id}")])
                .stdout(std::process::Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        // 组内进程可能已自行退出；未 reap 的 leader 仍占有 PID，故此时只接受
        // ps 证实无活成员的非零 kill 结果，权限错误且仍有活成员继续失败。
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "owned browser group kill exit={:?}, detail={}",
                output.status.code(),
                detail.trim().chars().take(160).collect::<String>()
            );
            if native_group_has_live_members(id).await? {
                return Err("owned browser process group termination failed".into());
            }
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while native_group_has_live_members(id).await? {
            if tokio::time::Instant::now() >= deadline {
                return Err("owned browser process group did not terminate".into());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = id;
        Err("native browser process-group supervisor requires Unix".into())
    }
}
async fn native_group_has_live_members(id: u32) -> TestResult<bool> {
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("/bin/ps")
            .args(["-axo", "pgid=,stat="])
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    if !output.status.success() {
        return Err("native process-group membership cannot be confirmed".into());
    }
    let listing = std::str::from_utf8(&output.stdout)?;
    Ok(listing.lines().any(|line| {
        let mut fields = line.split_ascii_whitespace();
        fields.next().and_then(|pgid| pgid.parse::<u32>().ok()) == Some(id)
            && fields.next().is_some_and(|state| !state.starts_with('Z'))
    }))
}
async fn native_leader_exited(pgid: u32) -> TestResult<bool> {
    // 仅查询本次 leader 的状态，不调用 waitpid/reap，也不读取命令行或环境。
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("/bin/ps")
            .args(["-p", &pgid.to_string(), "-o", "stat="])
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    let state = std::str::from_utf8(&output.stdout)?.trim();
    if !output.status.success() || state.is_empty() || state.len() > 32 {
        return Err("native leader ownership/status cannot be confirmed before reap".into());
    }
    Ok(state.starts_with('Z'))
}
impl NativeChild {
    fn child_mut(&mut self) -> TestResult<&mut tokio::process::Child> {
        self.child
            .as_mut()
            .ok_or_else(|| "native child already reaped".into())
    }
    async fn finish(&mut self) -> TestResult<std::process::ExitStatus> {
        // 必须在第一次 wait/try_wait 之前终止组；即便父已退出，其 PID 仍由本句柄占用。
        kill_native_group(self.pgid).await?;
        let status = tokio::time::timeout(BUDGET, self.child_mut()?.wait()).await??;
        self.child.take();
        Ok(status)
    }
}
impl Drop for NativeChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pgid = self.pgid;
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let _cleanup = runtime.spawn(async move {
                    let _ = kill_native_group(pgid).await;
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(BUDGET, child.wait()).await;
                });
            }
        }
    }
}
async fn read_native_output(reader: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    reader.take(65_537).read_to_end(&mut bytes).await?;
    Ok(bytes)
}
const NATIVE_REQUIRED: &[&str] = &[
    "passed",
    "complete",
    "owner_native_cookie_origin",
    "session_cookie_httponly",
    "cross_owner_rejected",
    "anonymous_rejected",
    "url_credential_rejected",
    "connection_limit_rejected",
    "client_publication_closed",
    "slot_released",
    "foreign_origin_rejected",
    "expiry_survived_until_lower_bound",
    "expiry_closed",
    "expired_session_rejected",
    "owner_controlled_shutdown_closed",
];
fn native_assertions_passed(report: &serde_json::Value) -> bool {
    if report.get("passed").and_then(serde_json::Value::as_bool) != Some(true)
        || report.get("complete").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return false;
    }
    NATIVE_REQUIRED
        .iter()
        .all(|name| report.get(*name).and_then(serde_json::Value::as_bool) == Some(true))
}
fn native_report(stdout: &[u8], stderr: &[u8]) -> TestResult<serde_json::Value> {
    if stdout.len() > 65_536 || stderr.len() > 65_536 {
        return Err("native browser output exceeded its safe bound".into());
    }
    let report = stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .rfind(|value| value.get("passed").is_some());
    let Some(report) = report else {
        let missing = String::from_utf8_lossy(stderr);
        return Err(
            if missing.contains("ModuleNotFoundError") && missing.contains("playwright") {
                "native browser prerequisite unavailable: Python playwright is not installed"
            } else {
                "native browser prerequisite/runtime failed before producing a structured report"
            }
            .into(),
        );
    };
    // 只保留已知非秘密元数据；绝不转储子进程 stderr、完整 JSON 或 traceback。
    let mut safe = serde_json::json!({});
    for name in NATIVE_REQUIRED {
        safe[*name] = serde_json::Value::Bool(
            report.get(*name).and_then(serde_json::Value::as_bool) == Some(true),
        );
    }
    for (name, allowed) in [
        (
            "phase",
            &[
                "login",
                "owner_open",
                "cross_owner",
                "anonymous",
                "query_credential",
                "connection_limit",
                "slot_reopen",
                "foreign_origin",
                "expiry",
                "expired_session",
                "shutdown",
                "finished",
            ][..],
        ),
        (
            "failure_class",
            &[
                "AssertionError",
                "TimeoutError",
                "Error",
                "ValueError",
                "ModuleNotFoundError",
            ][..],
        ),
        ("browser", &["chromium"][..]),
    ] {
        if let Some(value) = report[name]
            .as_str()
            .filter(|value| allowed.contains(value))
        {
            safe[name] = value.into();
        }
    }
    if let Some(version) = report["version"].as_str().filter(|value| {
        value.len() < 40
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
    }) {
        safe["version"] = version.into();
    }
    Ok(safe)
}

#[allow(clippy::too_many_lines)] // 同一宿主闭环协调秘密 fixture、浏览器、停机握手与有界回收。
async fn run_native_fixture(fixture: &mut Fixture) -> TestResult {
    use std::io::Write;
    let backend = fixture
        .app
        .app()?
        .state()
        .resolve::<Db>()
        .await?
        .primary_connection()?
        .get_database_backend();
    let foreign_listener = TcpListener::bind("127.0.0.1:0").await?;
    let foreign_origin = format!("http://{}", foreign_listener.local_addr()?);
    let router = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            axum::response::Html("<!doctype html><title>本测试的另一 Origin</title>")
        }),
    );
    let (foreign_stop, stopped) = oneshot::channel();
    let mut foreign = ListenerTask(tokio::spawn(async move {
        axum::serve(foreign_listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    }));
    let directory = tempfile::tempdir_in("/tmp")?;
    let path = directory.path().join("fixture.json");
    let marker = directory.path().join("shutdown-ready");
    let config = serde_json::json!({
        "origin":fixture.origin,"article_id":fixture.id,
        "owner":{"email":fixture.owner_email,"password":fixture.password},
        "other":{"email":fixture.other_email,"password":fixture.password},
        "cookie_name":COOKIE,"session_ttl_secs":15,
        "foreign_origin":foreign_origin,"shutdown_ready_path":marker,
    });
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(&serde_json::to_vec(&config)?)?;
    file.flush()?;
    drop(file);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("modules/example/tests/browser_websocket_native.py");
    let mut command = tokio::process::Command::new("python3");
    command
        .kill_on_drop(true)
        .arg(script)
        .arg("--fixture")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let spawned = command
        .spawn()
        .map_err(|error| format!("native browser Python cannot start: {error}"))?;
    let pgid = spawned.id().ok_or("native child PID unavailable")?;
    let mut child = NativeChild {
        child: Some(spawned),
        pgid,
    };
    let stdout = child
        .child_mut()?
        .stdout
        .take()
        .ok_or("native stdout missing")?;
    let stderr = child
        .child_mut()?
        .stderr
        .take()
        .ok_or("native stderr missing")?;
    let stdout = tokio::spawn(read_native_output(stdout));
    let stderr = tokio::spawn(read_native_output(stderr));
    let mut shutdown_seen = false;
    let outcome = tokio::time::timeout(Duration::from_secs(170), async {
        loop {
            if !shutdown_seen && marker.exists() {
                shutdown_seen = true;
                fixture
                    .app
                    .shutdown(tokio::time::Instant::now() + BUDGET)
                    .await?;
                if let Some(stop) = fixture.stop.take() {
                    let _ = stop.send(());
                }
            }
            // stdout EOF 仅表示本脚本已结束输出；不 reap leader，清理组后再 wait。
            if stdout.is_finished() && native_leader_exited(child.pgid).await? {
                break Ok::<_, TestError>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    // 正常与异常路径都先清本组，再 reap；不根据父已退出而跳过浏览器子进程。
    let reaped = child.finish().await;
    drop(child);
    let output = tokio::time::timeout(BUDGET, stdout).await???;
    let errors = tokio::time::timeout(BUDGET, stderr).await???;
    let foreign_signal = foreign_stop.send(());
    let foreign_join = tokio::time::timeout(BUDGET, &mut foreign.0).await;
    let report = native_report(&output, &errors);
    if let Ok(ref safe_report) = report {
        eprintln!("native-browser-runtime partial-safe-report {safe_report}");
    }
    outcome??;
    let status = reaped?;
    let mut report = report?;
    report["database_backend"] = format!("{backend:?}").into();
    eprintln!("native-browser-runtime {report}");
    let assertions_passed = native_assertions_passed(&report);
    if !status.success() || !assertions_passed {
        return Err(format!(
            "native browser acceptance failed: exit={:?}, safe_report={report}",
            status.code()
        )
        .into());
    }
    if !shutdown_seen {
        return Err("native browser did not exercise owner-controlled shutdown".into());
    }
    foreign_signal.map_err(|()| "foreign Origin listener stopped unexpectedly")?;
    foreign_join???;
    Ok(())
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL、已安装 Python Playwright/Chromium；必须显式运行，不以缺依赖跳过"]
async fn native_browser_runtime_session_websocket_acceptance() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut fixture = Fixture::create_with_listener(15, Some(listener)).await?;
    let outcome = run_native_fixture(&mut fixture).await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

#[test]
fn native_report_requires_every_explicit_true_boolean() -> TestResult {
    let mut valid = serde_json::json!({});
    for name in NATIVE_REQUIRED {
        valid[*name] = true.into();
    }
    let parse = |value: &serde_json::Value| -> TestResult<bool> {
        Ok(native_assertions_passed(&native_report(
            &serde_json::to_vec(value)?,
            b"",
        )?))
    };
    assert!(parse(&valid)?);
    for name in NATIVE_REQUIRED {
        for invalid in [
            serde_json::Value::Null,
            "true".into(),
            false.into(),
            1.into(),
        ] {
            let mut report = valid.clone();
            report[*name] = invalid;
            assert!(
                !parse(&report)?,
                "non-boolean or false acceptance field was accepted"
            );
        }
        let mut report = valid.clone();
        report
            .as_object_mut()
            .ok_or("report not object")?
            .remove(*name);
        // 没有 passed 的输入连最终报告都不能构成，仍必须拒绝而非成功。
        assert!(
            !matches!(parse(&report), Ok(true)),
            "missing acceptance field was accepted"
        );
    }
    Ok(())
}
