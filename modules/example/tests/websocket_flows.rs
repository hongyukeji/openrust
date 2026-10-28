//! 两次真实 TCP 协议握手验证文章 WebSocket；跨 App 不等于多 OS 进程集群。
#![cfg(all(
    feature = "auth",
    feature = "realtime",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::models::article;
use figment::{Figment, providers::Serialized};
use futures_util::{SinkExt, StreamExt};
use openrust::Db;
use openrust::auth::JwtTokens;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum;
use openrust::testing::{TestApp, TestDatabase};
use std::{net::SocketAddr, time::Duration};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Error as WsError, Message, client::IntoClientRequest},
};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult<T = ()> = std::result::Result<T, TestError>;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
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
fn sources(database: &str, ttl: u64, redis: Option<(&str, &str)>) -> Figment {
    let mut framework = openrust::config::FrameworkConfig::default();
    framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
        "app":{"name":"example-cluster","env":"test"},"database":{"url":database},
        "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
        "auth":{"jwt":{"issuer":"http://example.test","audience":"websocket-tests","active_key_id":"test","keys":{"test":key},"access_ttl_secs":ttl}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
        "realtime":{"max_connections":1,"max_message_bytes":256,"heartbeat_interval_secs":1,"allowed_origins":[ORIGIN]}
    })));
    if let Some((url, namespace)) = redis {
        source.merge(Serialized::defaults(
            serde_json::json!({"realtime":{"redis_url":url,"namespace":namespace}}),
        ))
    } else {
        source
    }
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
struct Fixture {
    node: Node,
    database: TestDatabase,
    id: i64,
    owner: String,
    other: String,
}
impl Fixture {
    async fn create(ttl: u64, redis: Option<(&str, &str)>) -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping article WebSocket test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let node = Node::start(sources(database.url(), ttl, redis)).await?;
        let db = node.app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let users = db.quoted_table_name("users")?;
        for id in [1_i64, 2] {
            let sql = format!(
                "INSERT INTO {users} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'socket-{id}@example.test','not-used',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"
            );
            connection
                .execute_raw(Statement::from_string(
                    connection.get_database_backend(),
                    sql,
                ))
                .await?;
        }
        let model = article::ActiveModel {
            owner_id: Set(1),
            title: Set("Socket article".into()),
            body: Set("Public body".into()),
            ..Default::default()
        }
        .insert(&connection)
        .await?;
        let jwt = node.app.app()?.state().resolve::<JwtTokens>().await?;
        Ok(Some(Self {
            owner: jwt.issue_access_token(1)?,
            other: jwt.issue_access_token(2)?,
            node,
            database,
            id: model.id,
        }))
    }
    async fn close(self) -> TestResult {
        let result = self.node.close().await;
        let database = self.database.close().await;
        result?;
        database?;
        Ok(())
    }
}
async fn handshake(
    node: &Node,
    id: i64,
    token: Option<&str>,
    origin: Option<&str>,
) -> std::result::Result<Socket, WsError> {
    let mut request =
        format!("ws://{}/api/example/articles/{id}/socket", node.address).into_client_request()?;
    if let Some(token) = token {
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().map_err(|_| {
                WsError::Io(std::io::Error::other("test authorization header invalid"))
            })?,
        );
    }
    if let Some(origin) = origin {
        request.headers_mut().insert(
            "origin",
            origin
                .parse()
                .map_err(|_| WsError::Io(std::io::Error::other("test origin header invalid")))?,
        );
    }
    let (socket, response) = connect_async(request).await?;
    assert_eq!(response.status().as_u16(), 101);
    Ok(socket)
}
async fn rejected(
    node: &Node,
    id: i64,
    token: Option<&str>,
    origin: Option<&str>,
    status: u16,
) -> TestResult {
    let result = tokio::time::timeout(BUDGET, handshake(node, id, token, origin)).await?;
    match result {
        Err(WsError::Http(response)) => assert_eq!(response.status().as_u16(), status),
        _ => return Err("unexpected WebSocket handshake result".into()),
    }
    Ok(())
}
async fn open(node: &Node, id: i64, token: &str) -> TestResult<Socket> {
    Ok(tokio::time::timeout(BUDGET, handshake(node, id, Some(token), Some(ORIGIN))).await??)
}
async fn data(socket: &mut Socket) -> TestResult<serde_json::Value> {
    tokio::time::timeout(BUDGET, async {
        loop {
            match socket.next().await.ok_or("socket closed before data")?? {
                Message::Text(text) => return Ok(serde_json::from_str(text.as_str())?),
                Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
                Message::Close(_) => return Err("socket closed before data".into()),
                _ => {}
            }
        }
    })
    .await?
}
async fn closed(socket: &mut Socket) -> TestResult {
    tokio::time::timeout(BUDGET, async {
        loop {
            match socket.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => return Ok::<_, TestError>(()),
                Some(Ok(Message::Ping(payload))) => {
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    })
    .await?
}
async fn reopen(node: &Node, id: i64, token: &str) -> TestResult<Socket> {
    tokio::time::timeout(BUDGET, async {
        loop {
            match handshake(node, id, Some(token), Some(ORIGIN)).await {
                Ok(socket) => return Ok::<_, TestError>(socket),
                Err(WsError::Http(response)) if response.status().as_u16() == 503 => {
                    tokio::task::yield_now().await;
                }
                Err(error) => return Err(Box::new(error) as TestError),
            }
        }
    })
    .await?
}
async fn announce(node: &Node, id: i64, token: &str, local: u64) -> TestResult {
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!(
            "http://{}/api/example/articles/{id}/announce",
            node.address
        ))
        .bearer_auth(token)
        .timeout(BUDGET)
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 200);
    let response: serde_json::Value = response.json().await?;
    assert_eq!(response["data"]["local_sessions_queued"], local);
    Ok(())
}

#[tokio::test]
async fn websocket_real_tcp_upgrade_authorization_frames_and_disconnect_release() -> TestResult {
    let Some(fixture) = Fixture::create(300, None).await? else {
        return Ok(());
    };
    let result = async {
        rejected(&fixture.node, fixture.id, None, Some(ORIGIN), 401).await?;
        rejected(
            &fixture.node,
            fixture.id,
            Some(&fixture.other),
            Some(ORIGIN),
            404,
        )
        .await?;
        rejected(
            &fixture.node,
            fixture.id,
            Some(&fixture.owner),
            Some("https://evil.test"),
            403,
        )
        .await?;
        let mut socket = open(&fixture.node, fixture.id, &fixture.owner).await?;
        rejected(
            &fixture.node,
            fixture.id,
            Some(&fixture.owner),
            Some(ORIGIN),
            503,
        )
        .await?;
        announce(&fixture.node, fixture.id, &fixture.owner, 1).await?;
        let event = data(&mut socket).await?;
        assert_eq!(event["article_id"], fixture.id);
        assert_eq!(event["event"], "article.announced");
        tokio::time::timeout(
            BUDGET,
            socket.send(Message::Text("subscribe:arbitrary-channel".into())),
        )
        .await??;
        closed(&mut socket).await?;
        drop(socket);
        let mut socket = reopen(&fixture.node, fixture.id, &fixture.owner).await?;
        tokio::time::timeout(BUDGET, socket.send(Message::Binary(vec![0; 4096].into()))).await??;
        closed(&mut socket).await?;
        drop(socket);
        let socket = reopen(&fixture.node, fixture.id, &fixture.owner).await?;
        drop(socket);
        let mut socket = reopen(&fixture.node, fixture.id, &fixture.owner).await?;
        fixture
            .node
            .app
            .app()?
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await?;
        closed(&mut socket).await?;
        Ok::<_, TestError>(())
    }
    .await;
    let cleanup = fixture.close().await;
    result?;
    cleanup
}
#[tokio::test]
async fn websocket_connection_ends_at_verified_jwt_expiration() -> TestResult {
    let Some(fixture) = Fixture::create(3, None).await? else {
        return Ok(());
    };
    let result = async {
        let mut socket = open(&fixture.node, fixture.id, &fixture.owner).await?;
        closed(&mut socket).await?;
        rejected(
            &fixture.node,
            fixture.id,
            Some(&fixture.owner),
            Some(ORIGIN),
            401,
        )
        .await?;
        Ok::<_, TestError>(())
    }
    .await;
    let cleanup = fixture.close().await;
    result?;
    cleanup
}
#[cfg(feature = "realtime-redis")]
#[tokio::test]
async fn websocket_receives_redis_announcement_from_independent_application() -> TestResult {
    let redis = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let namespace = format!("example-ws-{}", uuid::Uuid::now_v7());
    let Some(fixture) = Fixture::create(300, Some((&redis, &namespace))).await? else {
        return Ok(());
    };
    let second = Node::start(sources(
        fixture.database.url(),
        300,
        Some((&redis, &namespace)),
    ))
    .await?;
    let result = async {
        super::realtime_cluster::wait_subscribers(&redis, &namespace, 2).await?;
        let mut socket = open(&fixture.node, fixture.id, &fixture.owner).await?;
        announce(&second, fixture.id, &fixture.owner, 0).await?;
        assert_eq!(data(&mut socket).await?["article_id"], fixture.id);
        fixture
            .node
            .app
            .app()?
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await?;
        closed(&mut socket).await?;
        Ok::<_, TestError>(())
    }
    .await;
    let cleanup_second = second.close().await;
    let cleanup = fixture.close().await;
    let listeners = super::realtime_cluster::wait_subscribers(&redis, &namespace, 0).await;
    result?;
    cleanup_second?;
    cleanup?;
    listeners?;
    Ok(())
}
