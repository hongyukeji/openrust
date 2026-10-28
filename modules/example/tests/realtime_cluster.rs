//! 两个独立应用实例经真实 Redis transport 跨 TCP/SSE 投递。
//! 这不是两个 OS 进程，也不代替 R14 的多进程与故障拓扑验收。
#![cfg(all(
    feature = "auth",
    feature = "realtime-redis",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::models::article;
use figment::{Figment, providers::Serialized};
use openrust::Db;
use openrust::auth::JwtTokens;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum;
use openrust::realtime::Realtime;
use openrust::testing::{TestApp, TestDatabase};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult<T = ()> = std::result::Result<T, TestError>;
const BUDGET: Duration = Duration::from_secs(10);
const ORIGIN: &str = "https://example.test";

struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
fn sources(database: &str, redis: &str, namespace: &str) -> Figment {
    let mut config = openrust::config::FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    Figment::from(Serialized::defaults(config)).merge(Serialized::defaults(serde_json::json!({
        "app":{"name":"example-cluster","env":"test"},
        "database":{"url":database},
        "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
        "auth":{"jwt":{"issuer":"http://example.test","audience":"realtime-cluster","active_key_id":"test","keys":{"test":key}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
        "realtime":{"namespace":namespace,"redis_url":redis,"max_connections":1,"allowed_origins":[ORIGIN],"heartbeat_interval_secs":1}
    })))
}
struct Node {
    app: TestApp<crate::bootstrap::Server>,
    address: SocketAddr,
    stop: oneshot::Sender<()>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl Node {
    async fn start(source: Figment) -> TestResult<Self> {
        let app = TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        let router = openrust::http::build_router(app.app()?)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
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
        Ok(Self {
            app,
            address,
            stop,
            server,
        })
    }
    async fn close(mut self) -> TestResult {
        let result = self
            .app
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await;
        let _ = self.stop.send(());
        tokio::time::timeout(BUDGET, self.server).await???;
        result?;
        Ok(())
    }
}

/// 测试客户端只解码有界 HTTP/1.1 chunk；不把原始网络分片误当完整 SSE 事件。
struct WireResponse {
    reader: BufReader<TcpStream>,
    status: u16,
    chunked: bool,
    length: Option<usize>,
    content_type: String,
    pending: String,
}
impl WireResponse {
    async fn open(
        address: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
    ) -> TestResult<Self> {
        let mut stream = TcpStream::connect(address).await?;
        let auth = token.map_or_else(String::new, |token| {
            format!("Authorization: Bearer {token}\r\n")
        });
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: {ORIGIN}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let status = line
            .split_ascii_whitespace()
            .nth(1)
            .ok_or("HTTP status missing")?
            .parse()?;
        let (mut chunked, mut length, mut content_type, mut bytes) =
            (false, None, String::new(), 0);
        loop {
            line.clear();
            let count = reader.read_line(&mut line).await?;
            bytes += count;
            if count == 0 || bytes > 16 * 1024 {
                return Err("HTTP headers invalid".into());
            }
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                match name.to_ascii_lowercase().as_str() {
                    "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
                    "content-length" => length = Some(value.trim().parse::<usize>()?),
                    "content-type" => content_type = value.trim().to_owned(),
                    _ => {}
                }
            }
        }
        Ok(Self {
            reader,
            status,
            chunked,
            length,
            content_type,
            pending: String::new(),
        })
    }
    async fn chunk(&mut self) -> TestResult<Option<Vec<u8>>> {
        if !self.chunked {
            return Err("SSE response is not chunked HTTP/1.1".into());
        }
        let mut line = String::new();
        if self.reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let length = usize::from_str_radix(
            line.trim().split(';').next().ok_or("chunk size missing")?,
            16,
        )?;
        if length == 0 {
            return Ok(None);
        }
        if length > 64 * 1024 {
            return Err("HTTP chunk exceeds test budget".into());
        }
        let mut bytes = vec![0; length];
        self.reader.read_exact(&mut bytes).await?;
        let mut ending = [0; 2];
        self.reader.read_exact(&mut ending).await?;
        if ending != *b"\r\n" {
            return Err("HTTP chunk ending invalid".into());
        }
        Ok(Some(bytes))
    }
    async fn json(mut self) -> TestResult<serde_json::Value> {
        let mut bytes = Vec::new();
        if self.chunked {
            while let Some(chunk) = self.chunk().await? {
                bytes.extend(chunk);
                if bytes.len() > 64 * 1024 {
                    return Err("HTTP body exceeds test budget".into());
                }
            }
        } else {
            let length = self.length.ok_or("HTTP response length missing")?;
            if length > 64 * 1024 {
                return Err("HTTP body exceeds test budget".into());
            }
            bytes.resize(length, 0);
            self.reader.read_exact(&mut bytes).await?;
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    async fn event(&mut self) -> TestResult<serde_json::Value> {
        loop {
            while let Some(end) = self.pending.find("\n\n") {
                let frame = self.pending[..end].to_owned();
                self.pending.drain(..end + 2);
                for line in frame.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        return Ok(serde_json::from_str(data.trim())?);
                    }
                }
            }
            let bytes = self.chunk().await?.ok_or("SSE closed before event")?;
            self.pending.push_str(std::str::from_utf8(&bytes)?);
            if self.pending.len() > 64 * 1024 {
                return Err("SSE event exceeds test budget".into());
            }
        }
    }
}
async fn open(
    node: &Node,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> TestResult<WireResponse> {
    tokio::time::timeout(
        BUDGET,
        WireResponse::open(node.address, method, path, token),
    )
    .await?
}
async fn announce(node: &Node, article: i64, token: &str, local: u64) -> TestResult {
    let response = open(
        node,
        "POST",
        &format!("/api/example/articles/{article}/announce"),
        Some(token),
    )
    .await?;
    assert_eq!(response.status, 200);
    let json = tokio::time::timeout(BUDGET, response.json()).await??;
    assert_eq!(json["data"]["local_sessions_queued"], local);
    Ok(())
}
pub(super) async fn wait_subscribers(redis: &str, namespace: &str, expected: u64) -> TestResult {
    tokio::time::timeout(BUDGET, async {
        let client = redis::Client::open(redis).map_err(|_| "Redis test URL invalid")?;
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| "Redis test connection failed")?;
        let topic = format!("openrust:realtime:v1:example-cluster:{namespace}");
        loop {
            let counts: Vec<(String, u64)> = redis::cmd("PUBSUB")
                .arg("NUMSUB")
                .arg(&topic)
                .query_async(&mut connection)
                .await
                .map_err(|_| "Redis subscriber observation failed")?;
            if counts.first().is_some_and(|(_, count)| *count == expected) {
                return Ok::<_, TestError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
async fn exercise(a: &Node, b: &Node, ids: [i64; 2]) -> TestResult {
    let a_state = a.app.app()?.state();
    let b_state = b.app.app()?.state();
    assert!(!Arc::ptr_eq(
        &a_state.resolve::<Realtime>().await?,
        &b_state.resolve::<Realtime>().await?
    ));
    assert!(!Arc::ptr_eq(
        &a_state.resolve::<Db>().await?,
        &b_state.resolve::<Db>().await?
    ));
    let tokens = a_state.resolve::<JwtTokens>().await?;
    let owner = tokens.issue_access_token(1)?;
    let other = tokens.issue_access_token(2)?;
    let events = format!("/api/example/articles/{}/events", ids[0]);
    assert_eq!(open(a, "GET", &events, None).await?.status, 401);
    assert_eq!(open(a, "GET", &events, Some(&other)).await?.status, 404);
    assert_eq!(
        open(
            b,
            "POST",
            &format!("/api/example/articles/{}/announce", ids[0]),
            Some(&other)
        )
        .await?
        .status,
        404
    );
    let mut owner_stream = open(a, "GET", &events, Some(&owner)).await?;
    assert_eq!(owner_stream.status, 200);
    assert_eq!(owner_stream.content_type, "text/event-stream");
    let mut other_stream = open(
        b,
        "GET",
        &format!("/api/example/articles/{}/events", ids[1]),
        Some(&other),
    )
    .await?;
    assert_eq!(other_stream.status, 200);
    announce(b, ids[1], &other, 1).await?;
    assert_eq!(
        tokio::time::timeout(BUDGET, other_stream.event()).await??["article_id"],
        ids[1]
    );
    // B 没有 owner1 本地会话；A 收到只能经共享 Redis transport。
    announce(b, ids[0], &owner, 0).await?;
    let event = tokio::time::timeout(BUDGET, owner_stream.event()).await??;
    assert_eq!(event["article_id"], ids[0]);
    assert_eq!(event["event"], "article.announced");
    announce(b, ids[1], &other, 1).await?;
    assert_eq!(
        tokio::time::timeout(BUDGET, other_stream.event()).await??["article_id"],
        ids[1]
    );
    // max_connections=1 的 admission 在真实客户端断连后必须归还。
    drop(owner_stream);
    let mut replacement = tokio::time::timeout(BUDGET, async {
        loop {
            let response = open(a, "GET", &events, Some(&owner)).await?;
            if response.status == 200 {
                return Ok::<_, TestError>(response);
            }
            assert_eq!(response.status, 503);
            tokio::task::yield_now().await;
        }
    })
    .await??;
    announce(b, ids[0], &owner, 0).await?;
    assert_eq!(
        tokio::time::timeout(BUDGET, replacement.event()).await??["article_id"],
        ids[0]
    );
    // 父 shutdown 必须结束仍由真实客户端持有的流，而不是依赖客户端先 drop。
    a.app
        .app()?
        .shutdown(tokio::time::Instant::now() + BUDGET)
        .await?;
    tokio::time::timeout(BUDGET, async {
        while replacement.chunk().await?.is_some() {}
        Ok::<_, TestError>(())
    })
    .await??;
    drop(other_stream);
    Ok(())
}

#[tokio::test]
async fn article_announcements_cross_independent_apps_over_redis_and_tcp_sse() -> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping realtime transport test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let redis = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let namespace = format!("example-cluster-{}", uuid::Uuid::now_v7());
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let a = Node::start(sources(database.url(), &redis, &namespace)).await?;
    let b = Node::start(sources(database.url(), &redis, &namespace)).await?;
    let db = a.app.app()?.state().resolve::<Db>().await?;
    let connection = db.primary_connection()?;
    let users = db.quoted_table_name("users")?;
    let mut ids = [0; 2];
    for (index, id) in [1_i64, 2].into_iter().enumerate() {
        let sql = format!(
            "INSERT INTO {users} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'cluster-{id}@example.test','not-used',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"
        );
        connection
            .execute_raw(Statement::from_string(
                connection.get_database_backend(),
                sql,
            ))
            .await?;
        ids[index] = article::ActiveModel {
            owner_id: Set(id),
            title: Set("Realtime cluster".into()),
            body: Set("Public body".into()),
            ..Default::default()
        }
        .insert(&connection)
        .await?
        .id;
    }
    let outcome = async {
        wait_subscribers(&redis, &namespace, 2).await?;
        exercise(&a, &b, ids).await
    }
    .await;
    let close_a = a.close().await;
    let close_b = b.close().await;
    let listeners_closed = wait_subscribers(&redis, &namespace, 0).await;
    drop(connection);
    drop(db);
    let close_database = database.close().await;
    outcome?;
    close_a?;
    close_b?;
    listeners_closed?;
    close_database?;
    Ok(())
}
