//! 摘要业务使用真实本机 TCP 上游验证总超时与取消；RP 为生产 `TestApp` oneshot。
#![cfg(all(
    feature = "auth",
    feature = "http-client",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::models::article;
use figment::{Figment, providers::Serialized};
use openrust::auth::{AuthFlows, JwtTokens};
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Db, Error, Result};
use std::{io, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

const WAIT: Duration = Duration::from_secs(3);
const PRIVATE: &str = "SECRET_UPSTREAM_SENTINEL";
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum Stall {
    Headers,
    Body,
}
struct Upstream {
    origin: String,
    ready: Option<oneshot::Receiver<()>>,
    closed: Option<oneshot::Receiver<()>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<io::Result<()>>>,
}
impl Upstream {
    async fn start(stall: Stall) -> TestResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}/", listener.local_addr()?);
        let (ready_tx, ready) = oneshot::channel();
        let (closed_tx, closed) = oneshot::channel();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut stream, _) = tokio::select! {
                accepted = listener.accept() => accepted?,
                _ = &mut stopped => return Ok(()),
            };
            let mut request = Vec::new();
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut bytes = [0_u8; 1024];
                let read = tokio::select! {
                    read = stream.read(&mut bytes) => read?,
                    _ = &mut stopped => return Ok(()),
                };
                if read == 0 || request.len() + read > 16 * 1024 {
                    return Err(io::Error::other(
                        "upstream fixture request incomplete or too large",
                    ));
                }
                request.extend_from_slice(&bytes[..read]);
            }
            // 响应只发送前缀，不能因非法完整 JSON 提前结束；等待实际客户端关闭连接。
            if matches!(stall, Stall::Body) {
                let prefix = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1024\r\nX-Upstream-Private: {PRIVATE}\r\n\r\n{{\"summary\":\"{PRIVATE}"
                );
                stream.write_all(prefix.as_bytes()).await?;
                stream.flush().await?;
            }
            let _ = ready_tx.send(());
            let mut bytes = [0_u8; 128];
            loop {
                let read = tokio::select! {
                    read = stream.read(&mut bytes) => read,
                    _ = &mut stopped => return Ok(()),
                };
                match read {
                    Ok(0) => {
                        let _ = closed_tx.send(());
                        return Ok(());
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::ConnectionReset
                                | io::ErrorKind::BrokenPipe
                                | io::ErrorKind::UnexpectedEof
                        ) =>
                    {
                        let _ = closed_tx.send(());
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                    Ok(_) => {}
                }
            }
        });
        Ok(Self {
            origin,
            ready: Some(ready),
            closed: Some(closed),
            stop: Some(stop),
            task: Some(task),
        })
    }
    async fn wait_ready(&mut self) -> TestResult {
        tokio::time::timeout(
            WAIT,
            self.ready
                .take()
                .ok_or("upstream readiness already observed")?,
        )
        .await??;
        Ok(())
    }
    async fn wait_closed(&mut self) -> TestResult {
        tokio::time::timeout(
            WAIT,
            self.closed
                .take()
                .ok_or("upstream closure already observed")?,
        )
        .await??;
        Ok(())
    }
    async fn close(mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            if let Ok(result) = tokio::time::timeout(WAIT, &mut task).await {
                result??;
            } else {
                task.abort();
                let _ = tokio::time::timeout(WAIT, task).await;
                return Err("upstream fixture join exceeded its deadline".into());
            }
        }
        Ok(())
    }
}
impl Drop for Upstream {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // 仅兜底取消本测试拥有的任务；正常路径必须显式 close 并 join。
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

struct RequestTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for RequestTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: Arc<TestApp<crate::bootstrap::Server>>,
    database: TestDatabase,
    article_id: i64,
    token: String,
}
impl Fixture {
    async fn create(origin: &str) -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping real summary timeout test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source = Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"},"database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"summary-timeout","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "example":{"article_summary_origin":origin}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        let auth = app.app()?.state().resolve::<AuthFlows>().await?;
        let email = "summary-timeout@example.test";
        let password = "an isolated summary fixture password";
        let verification = auth
            .register(email, password, "summary-timeout")
            .await?
            .ok_or("verification code missing")?;
        assert!(
            auth.verify_email(email, verification.code(), "summary-timeout")
                .await?
        );
        let owner = auth
            .login(email, password, "summary-timeout")
            .await?
            .user_id();
        let token = app
            .app()?
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(owner)?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let model = article::ActiveModel {
            owner_id: Set(owner),
            title: Set("Summary timeout fixture".into()),
            body: Set("A local article".into()),
            private_note_ciphertext: Set(None),
            ..Default::default()
        }
        .insert(&db.connection()?)
        .await?;
        Ok(Some(Self {
            app: Arc::new(app),
            database,
            article_id: model.id,
            token,
        }))
    }
    fn request(&self) -> Result<Request<Body>> {
        Request::builder()
            .uri(format!("/api/example/articles/{}/summary", self.article_id))
            .header("host", "localhost")
            .header("authorization", format!("Bearer {}", self.token))
            .extension(ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                45678,
            ))))
            .body(Body::empty())
            .map_err(Error::internal)
    }
    async fn close(self) -> TestResult {
        let mut app =
            Arc::try_unwrap(self.app).map_err(|_| "summary request still owns its app")?;
        app.shutdown(tokio::time::Instant::now() + WAIT).await?;
        self.database.close().await?;
        Ok(())
    }
}

#[tokio::test]
async fn stalled_headers_and_body_return_safe_502_and_release_real_upstream_connection()
-> TestResult {
    for stall in [Stall::Headers, Stall::Body] {
        let mut upstream = Upstream::start(stall).await?;
        let Some(fixture) = Fixture::create(&upstream.origin).await? else {
            upstream.close().await?;
            return Ok(());
        };
        let app = Arc::clone(&fixture.app);
        let request = fixture.request()?;
        let started = tokio::time::Instant::now();
        // 业务总时限固定 5 秒；测试预算较大，避免 TestApp 的默认 5 秒先截断安全响应。
        let mut response = RequestTask(tokio::spawn(async move {
            app.request_with_limits(request, 16 * 1024, Duration::from_secs(9))
                .await
        }));
        upstream.wait_ready().await?;
        let response = tokio::time::timeout(Duration::from_secs(9), &mut response.0).await???;
        assert!(
            started.elapsed() >= Duration::from_secs(4),
            "must reach business timeout instead of failing immediately"
        );
        response.assert_status(StatusCode::BAD_GATEWAY)?;
        let body: serde_json::Value = response.json()?;
        assert_eq!(body["code"], "example_article_summary_upstream_failed");
        let body = std::str::from_utf8(response.body())?;
        assert!(!body.contains(PRIVATE));
        assert!(!body.contains(&upstream.origin));
        assert!(!body.contains("timed out") && !body.contains("reqwest"));
        upstream.wait_closed().await?;
        fixture.close().await?;
        upstream.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn cancelling_production_handler_drops_real_upstream_request_and_partial_body() -> TestResult
{
    for stall in [Stall::Headers, Stall::Body] {
        let mut upstream = Upstream::start(stall).await?;
        let Some(fixture) = Fixture::create(&upstream.origin).await? else {
            upstream.close().await?;
            return Ok(());
        };
        let app = Arc::clone(&fixture.app);
        let request = fixture.request()?;
        let mut request_task =
            RequestTask(tokio::spawn(async move { app.raw_response(request).await }));
        // 由上游实际读取请求并发送部分体后发信号，不能靠 sleep 猜测出站 future 已进入。
        upstream.wait_ready().await?;
        tokio::time::timeout(Duration::from_secs(2), async {
            request_task.0.abort();
            let joined = (&mut request_task.0).await;
            assert!(joined.is_err_and(|error| error.is_cancelled()));
            // 从取消起的 2 秒预算小于业务 5 秒时限，不等正常超时替代取消证据。
            upstream.wait_closed().await
        })
        .await??;
        fixture.close().await?;
        upstream.close().await?;
    }
    Ok(())
}
