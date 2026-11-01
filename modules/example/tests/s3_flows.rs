//! 两个独立应用通过真实 TCP HTTP 与 S3 共享附件；取消回归为进程内请求。
//! 不代表多进程集群、租户隔离或远端 multipart 清理验收。
#![cfg(all(
    feature = "auth",
    feature = "storage-s3",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::http::requests::attachments::MAX_ATTACHMENT_BYTES;
use super::models::{article, attachment_cleanup};
use super::services::attachments::ArticleAttachments;
use figment::{Figment, providers::Serialized};
use futures_util::StreamExt;
use openrust::auth::JwtTokens;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use openrust::storage::{Storage, StoragePath};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Db, Error, Result};
use std::{sync::Arc, time::Duration};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
pub(super) struct Fixture {
    pub(super) a: TestApp<crate::bootstrap::Server>,
    pub(super) b: TestApp<crate::bootstrap::Server>,
    pub(super) database: TestDatabase,
    pub(super) id: i64,
    pub(super) owner: String,
    pub(super) other: String,
    pub(super) observer: opendal::Operator,
}
impl Fixture {
    pub(super) async fn create() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Self::create_with_store(serde_json::json!({"driver":"memory"})).await
    }
    pub(super) async fn create_with_store(
        store: serde_json::Value,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Self::create_with_store_endpoint(store, None).await
    }

    async fn create_with_store_endpoint(
        store: serde_json::Value,
        app_endpoint: Option<&str>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        // 显式 ignore 测试缺少真实服务配置时失败，不把跳过算作验收。
        let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
        let endpoint = std::env::var("OPENRUST_TEST_S3_ENDPOINT")?;
        let bucket = std::env::var("OPENRUST_TEST_S3_BUCKET")?;
        let region = std::env::var("OPENRUST_TEST_S3_REGION")?;
        let access = std::env::var("OPENRUST_TEST_S3_ACCESS_KEY_ID")?;
        let secret = std::env::var("OPENRUST_TEST_S3_SECRET_ACCESS_KEY")?;
        let root = format!("r1e/{}", uuid::Uuid::now_v7());
        // 仅观测本 fixture 的保留暂存前缀；业务请求仍经公开 Provider 与生产路由。
        let observer = opendal::Operator::new(
            opendal::services::S3::default()
                .bucket(&bucket)
                .region(&region)
                .endpoint(&endpoint)
                .access_key_id(&access)
                .secret_access_key(&secret)
                .root(&root)
                .disable_config_load(),
        )?;
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source = Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"}, "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":store}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"s3-attachments","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "storage":{"default_disk":"shared","disks":{"shared":{"driver":"s3","bucket":bucket,"region":region,"endpoint":app_endpoint.unwrap_or(&endpoint),"access_key_id":access,"secret_access_key":secret,"root":root}}}
        })));
        let a = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source.clone()))?,
        )
        .build()
        .await?;
        let b = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        for app in [&a, &b] {
            let config = app
                .app()?
                .section::<openrust::config::StorageConfig>("storage")?;
            assert_eq!(config.default_disk, "shared");
            assert_eq!(
                config
                    .disks
                    .get("shared")
                    .and_then(|disk| disk.options.get("root")),
                Some(&serde_json::Value::String(root.clone()))
            );
        }
        let db = a.app()?.state().resolve::<Db>().await?;
        let model = article::ActiveModel {
            owner_id: Set(1),
            title: Set("S3 附件".into()),
            body: Set("共享存储".into()),
            ..Default::default()
        }
        .insert(&db.connection()?)
        .await?;
        let tokens = a.app()?.state().resolve::<JwtTokens>().await?;
        Ok(Self {
            owner: tokens.issue_access_token(1)?,
            other: tokens.issue_access_token(2)?,
            a,
            b,
            database,
            id: model.id,
            observer,
        })
    }
    pub(super) fn path(&self) -> String {
        format!("/api/example/articles/{}/attachment", self.id)
    }
    pub(super) fn object(&self) -> Result<StoragePath> {
        StoragePath::new(format!("example/articles/1/{}/attachment.txt", self.id))
    }
    async fn assert_missing(&self) -> TestResult {
        let storage = self.b.app()?.state().resolve::<Storage>().await?;
        let error = storage
            .default_disk()
            .await?
            .stat(&self.object()?)
            .await
            .err()
            .ok_or("partial object was visible")?;
        assert_eq!(error.code(), "storage_object_not_found");
        Ok(())
    }
    async fn assert_staging_cleaned(&self) -> TestResult {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let entries = self.observer.list(".staging/").await?;
                if entries.iter().all(|entry| entry.path().ends_with('/')) {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        Ok(())
    }
    pub(super) async fn close(mut self) -> TestResult {
        self.assert_staging_cleaned().await?;
        let storage = self.b.app()?.state().resolve::<Storage>().await?;
        // 仅清理本 fixture 唯一 key，绝不清空共享 bucket。
        storage
            .default_disk()
            .await?
            .delete(&self.object()?)
            .await?;
        self.a
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.b
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.database.close().await?;
        Ok(())
    }
}

#[cfg(feature = "http-client")]
struct CopyProxy {
    endpoint: String,
    reached: Arc<tokio::sync::Notify>,
    copy_completed: Arc<tokio::sync::Notify>,
    copy_status: Arc<tokio::sync::Mutex<Option<u16>>>,
    release: Arc<tokio::sync::Semaphore>,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "http-client")]
impl CopyProxy {
    #[allow(clippy::too_many_lines)] // 真实签名请求的时序控制与代理转发需保持同一测试 fixture。
    async fn start(upstream: String) -> std::io::Result<Self> {
        use openrust::http::axum::{Router, extract::State, routing::any};

        struct StateData {
            upstream: String,
            client: reqwest::Client,
            reached: Arc<tokio::sync::Notify>,
            copy_completed: Arc<tokio::sync::Notify>,
            copy_status: Arc<tokio::sync::Mutex<Option<u16>>>,
            release: Arc<tokio::sync::Semaphore>,
        }

        async fn forward(
            State(state): State<Arc<StateData>>,
            request: openrust::http::axum::extract::Request,
        ) -> openrust::http::axum::response::Response {
            use openrust::http::axum::body::to_bytes;

            let (parts, body) = request.into_parts();
            let is_copy = parts.headers.contains_key("x-amz-copy-source");
            let target = format!("{}{}", state.upstream.trim_end_matches('/'), parts.uri);
            let Ok(bytes) = to_bytes(body, 2 * 1024 * 1024).await else {
                return openrust::http::axum::response::Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::empty())
                    .unwrap_or_else(|_| {
                        openrust::http::axum::response::Response::new(Body::empty())
                    });
            };
            let outbound = state
                .client
                .request(parts.method, target)
                .headers(parts.headers)
                .body(bytes);
            match outbound.send().await {
                Ok(response) => {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let body = response.bytes().await.unwrap_or_default();
                    if is_copy {
                        let has_copy_result = body
                            .windows(b"<CopyObjectResult".len())
                            .any(|part| part == b"<CopyObjectResult");
                        let has_error = body.windows(b"<Error".len()).any(|part| part == b"<Error");
                        let result = if status == StatusCode::OK && has_copy_result && !has_error {
                            status.as_u16()
                        } else {
                            0
                        };
                        *state.copy_status.lock().await = Some(result);
                        state.copy_completed.notify_one();
                        state.reached.notify_one();
                        let Ok(_permit) = state.release.acquire().await else {
                            return openrust::http::axum::response::Response::builder()
                                .status(StatusCode::SERVICE_UNAVAILABLE)
                                .body(Body::empty())
                                .unwrap_or_else(|_| {
                                    openrust::http::axum::response::Response::new(Body::empty())
                                });
                        };
                    }
                    let mut builder =
                        openrust::http::axum::response::Response::builder().status(status);
                    for (name, value) in &headers {
                        builder = builder.header(name, value);
                    }
                    builder.body(Body::from(body)).unwrap_or_else(|_| {
                        openrust::http::axum::response::Response::new(Body::empty())
                    })
                }
                Err(_) => openrust::http::axum::response::Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::empty())
                    .unwrap_or_else(|_| {
                        openrust::http::axum::response::Response::new(Body::empty())
                    }),
            }
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let reached = Arc::new(tokio::sync::Notify::new());
        let copy_completed = Arc::new(tokio::sync::Notify::new());
        let copy_status = Arc::new(tokio::sync::Mutex::new(None));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let state = Arc::new(StateData {
            upstream,
            client: reqwest::Client::builder()
                .no_proxy()
                .build()
                .map_err(std::io::Error::other)?,
            reached: Arc::clone(&reached),
            copy_completed: Arc::clone(&copy_completed),
            copy_status: Arc::clone(&copy_status),
            release: Arc::clone(&release),
        });
        let router = Router::new().fallback(any(forward)).with_state(state);
        let task = tokio::spawn(async move {
            let _ = openrust::http::axum::serve(listener, router).await;
        });
        Ok(Self {
            endpoint: format!("http://{address}"),
            reached,
            copy_completed,
            copy_status,
            release,
            task,
        })
    }

    async fn wait_for_copy(&self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(15), self.reached.notified())
            .await
            .map_err(Error::internal)
    }

    async fn wait_for_successful_copy(&self) -> Result<u16> {
        tokio::time::timeout(Duration::from_secs(15), self.copy_completed.notified())
            .await
            .map_err(Error::internal)?;
        self.copy_status
            .lock()
            .await
            .ok_or_else(|| Error::internal(std::io::Error::other("CopyObject status missing")))
    }
}

#[cfg(feature = "http-client")]
impl Drop for CopyProxy {
    fn drop(&mut self) {
        self.release.add_permits(1);
        self.task.abort();
    }
}
fn request(method: &str, path: &str, token: &str, body: Body) -> Result<Request<Body>> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "text/plain")
        .body(body)
        .map_err(Error::internal)
}

pub(super) struct HttpServer {
    pub(super) origin: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl HttpServer {
    pub(super) async fn start(
        app: &TestApp<crate::bootstrap::Server>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let router = openrust::http::build_router(app.app()?)?;
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            openrust::http::axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        });
        Ok(Self {
            origin,
            stop: Some(stop),
            task,
        })
    }
    pub(super) async fn shutdown(mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        tokio::time::timeout(Duration::from_secs(5), &mut self.task).await???;
        Ok(())
    }
}
impl Drop for HttpServer {
    fn drop(&mut self) {
        // 断言或网络错误提前返回时也不遗留监听任务。
        self.task.abort();
    }
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 MinIO，显式配置 OPENRUST_TEST_S3_* 后运行"]
async fn s3_two_apps_share_owner_attachment_and_reject_invalid_streams() -> TestResult {
    let f = Fixture::create().await?;
    let a = HttpServer::start(&f.a).await?;
    let b = HttpServer::start(&f.b).await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let a_url = format!("{}{}", a.origin, f.path());
    let b_url = format!("{}{}", b.origin, f.path());
    let text = "共享的完整 UTF-8 文本🙂";
    let upload = client
        .put(&a_url)
        .bearer_auth(&f.owner)
        .header("content-type", "text/plain")
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter([
            Ok::<_, std::io::Error>(Bytes::from_static("共享的".as_bytes())),
            Ok(Bytes::from_static("完整 UTF-8 文本🙂".as_bytes())),
        ])))
        .send()
        .await?;
    if upload.status() != StatusCode::CREATED {
        let status = upload.status();
        let body: serde_json::Value = upload.json().await?;
        return Err(format!("S3 upload status {status}, code {}", body["code"]).into());
    }
    let downloaded = client.get(&b_url).bearer_auth(&f.owner).send().await?;
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(downloaded.bytes().await?.as_ref(), text.as_bytes());
    for method in [
        reqwest::Method::GET,
        reqwest::Method::PUT,
        reqwest::Method::DELETE,
    ] {
        let response = client
            .request(method, &b_url)
            .bearer_auth(&f.other)
            .header("content-type", "text/plain")
            .body("forbidden")
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    for bytes in [
        vec![b'x'; usize::try_from(MAX_ATTACHMENT_BYTES)? + 1],
        b"bad\0text".to_vec(),
        vec![0xf0, 0x9f],
        Vec::new(),
    ] {
        // 不声明长度，以分块传输验证实际流边界而非仅 Content-Length 拒绝。
        let chunks: Vec<_> = bytes
            .chunks(16 * 1024)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect();
        let response = client
            .put(&a_url)
            .bearer_auth(&f.owner)
            .header("content-type", "text/plain")
            .body(reqwest::Body::wrap_stream(futures_util::stream::iter(
                chunks,
            )))
            .send()
            .await?;
        assert!(matches!(
            response.status(),
            StatusCode::PAYLOAD_TOO_LARGE | StatusCode::UNPROCESSABLE_ENTITY
        ));
        let response = client.get(&b_url).bearer_auth(&f.owner).send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-content-type-options")
                .ok_or("missing nosniff")?,
            "nosniff"
        );
        assert_eq!(response.bytes().await?.as_ref(), text.as_bytes());
    }
    a.shutdown().await?;
    b.shutdown().await?;
    f.close().await
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 MinIO，显式配置 OPENRUST_TEST_S3_* 后运行"]
async fn s3_committed_delete_cancelled_request_is_reconciled_by_second_app() -> TestResult {
    let f = Fixture::create().await?;
    f.a.request(request(
        "PUT",
        &f.path(),
        &f.owner,
        Body::from("before delete"),
    )?)
    .await?
    .assert_status(StatusCode::CREATED)?;
    let service = f.a.app()?.state().resolve::<ArticleAttachments>().await?;
    let reached = Arc::new(tokio::sync::Notify::new());
    service.pause_before_cleanup(
        Arc::clone(&reached),
        Arc::new(tokio::sync::Semaphore::new(0)),
    )?;
    {
        let deletion = f.a.raw_response(request(
            "DELETE",
            &format!("/api/example/articles/{}", f.id),
            &f.owner,
            Body::empty(),
        )?);
        tokio::pin!(deletion);
        tokio::time::timeout(Duration::from_secs(5),async {
            tokio::select! {
                () = reached.notified() => Ok(()),
                result = &mut deletion => Err(format!("delete ended before pause: {:?}",result.map(|r|r.status()))),
            }
        }).await??;
    }
    let db = f.b.app()?.state().resolve::<Db>().await?;
    assert!(
        article::Entity::find_by_id(f.id)
            .one(&db.primary_connection()?)
            .await?
            .is_none()
    );
    assert!(
        attachment_cleanup::Entity::find_by_id(f.id)
            .one(&db.primary_connection()?)
            .await?
            .is_some()
    );
    let storage = f.b.app()?.state().resolve::<Storage>().await?;
    storage.default_disk().await?.stat(&f.object()?).await?;
    for _ in 0..2 {
        f.b.app()?
            .run_command("example", vec!["attachments".into(), "reconcile".into()])
            .await?;
    }
    f.assert_missing().await?;
    assert!(
        attachment_cleanup::Entity::find_by_id(f.id)
            .one(&db.primary_connection()?)
            .await?
            .is_none()
    );
    f.b.request(request("GET", &f.path(), &f.owner, Body::empty())?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    f.close().await
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 MinIO，显式配置 OPENRUST_TEST_S3_* 后运行"]
async fn s3_cancelled_http_upload_does_not_publish_partial_object() -> TestResult {
    let f = Fixture::create().await?;
    let reached = Arc::new(tokio::sync::Notify::new());
    let signal = Arc::clone(&reached);
    let body = Body::from_stream(
        futures_util::stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(b"partial"))
        })
        .chain(futures_util::stream::once(async move {
            signal.notify_one();
            std::future::pending::<std::result::Result<Bytes, std::io::Error>>().await
        })),
    );
    {
        let upload = f.a.raw_response(request("PUT", &f.path(), &f.owner, body)?);
        tokio::pin!(upload);
        tokio::time::timeout(Duration::from_secs(5),async {
            tokio::select! {
                () = reached.notified() => Ok(()),
                result = &mut upload => Err(format!("upload ended before body pause: {:?}",result.map(|r|r.status()))),
            }
        }).await??;
        f.assert_missing().await?;
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let disk =
                f.b.app()?
                    .state()
                    .resolve::<Storage>()
                    .await?
                    .default_disk()
                    .await?;
            match disk.stat(&f.object()?).await {
                Err(error) if error.code() == "storage_object_not_found" => {
                    break Ok::<(), Box<dyn std::error::Error>>(());
                }
                Err(error) => break Err(format!("storage stat failed: {}", error.code()).into()),
                Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    })
    .await??;
    f.close().await
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 MinIO，显式配置 OPENRUST_TEST_S3_* 后运行"]
async fn s3_cancelled_waiter_keeps_delete_blocked_until_publish_finishes() -> TestResult {
    let f = Fixture::create().await?;
    let attachments = f.a.app()?.state().resolve::<ArticleAttachments>().await?;
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    attachments.pause_after_publish(Arc::clone(&reached), Arc::clone(&release))?;
    let id = f.id;
    let mut upload = tokio::spawn(async move {
        attachments
            .upload(
                1,
                id,
                super::http::requests::attachments::AttachmentUpload::from_content_type(
                    "text/plain",
                )?,
                Body::from("complete S3 publication"),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::select! {
            () = reached.notified() => Ok(()),
            result = &mut upload => Err(format!("S3 upload ended before publish gate: {:?}", result.map(|value| value.map(|_| "published").map_err(|error| error.code().to_owned())))),
        }
    })
    .await??;
    f.a.app()?
        .state()
        .resolve::<Storage>()
        .await?
        .default_disk()
        .await?
        .stat(&f.object()?)
        .await?;
    upload.abort();
    assert!(upload.await.is_err());

    {
        let deletion = f.a.request(request(
            "DELETE",
            &format!("/api/example/articles/{id}"),
            &f.owner,
            Body::empty(),
        )?);
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut deletion)
                .await
                .is_err()
        );
        release.add_permits(1);
        deletion.as_mut().await?.assert_status(StatusCode::OK)?;
    }
    f.assert_missing().await?;
    f.close().await
}

#[cfg(feature = "http-client")]
#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 MinIO，显式配置 OPENRUST_TEST_S3_* 后运行"]
async fn s3_inflight_copy_keeps_delete_locked_until_late_publish_is_cleaned() -> TestResult {
    let upstream = std::env::var("OPENRUST_TEST_S3_ENDPOINT")?;
    let proxy = CopyProxy::start(upstream).await?;
    let f = Fixture::create_with_store_endpoint(
        serde_json::json!({"driver":"memory"}),
        Some(&proxy.endpoint),
    )
    .await?;
    let attachments = f.a.app()?.state().resolve::<ArticleAttachments>().await?;
    let id = f.id;
    let mut upload = tokio::spawn(async move {
        attachments
            .upload_with_test_deadlines(
                1,
                id,
                super::http::requests::attachments::AttachmentUpload::from_content_type(
                    "text/plain",
                )?,
                Body::from("copy remains in flight"),
                Duration::from_secs(30),
                Duration::from_millis(100),
            )
            .await
    });
    proxy.wait_for_copy().await?;
    let error = tokio::time::timeout(Duration::from_secs(5), &mut upload)
        .await??
        .err()
        .ok_or("upload waiter should time out while real CopyObject is held")?;
    assert_eq!(error.code(), "example_attachment_upload_timeout");

    {
        let deletion = f.a.request(request(
            "DELETE",
            &format!("/api/example/articles/{id}"),
            &f.owner,
            Body::empty(),
        )?);
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut deletion)
                .await
                .is_err(),
            "article deletion must wait for the in-flight publication transaction"
        );
        assert_eq!(
            proxy.wait_for_successful_copy().await?,
            StatusCode::OK.as_u16()
        );
        let storage = f.b.app()?.state().resolve::<Storage>().await?;
        let mut reader = storage
            .default_disk()
            .await?
            .open(&f.object()?, None)
            .await?;
        let mut published = Vec::new();
        while let Some(chunk) = reader.body.next().await {
            published.extend_from_slice(&chunk?);
        }
        assert_eq!(published, b"copy remains in flight");
        proxy.release.add_permits(1);
        deletion.as_mut().await?.assert_status(StatusCode::OK)?;
    }
    f.assert_missing().await?;
    f.assert_staging_cleaned().await?;
    f.close().await
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 MinIO，显式配置 OPENRUST_TEST_S3_* 后运行"]
async fn s3_pending_replacement_and_cancel_preserve_old_object_across_apps() -> TestResult {
    let f = Fixture::create().await?;
    let old = "完整旧对象";
    f.a.request(request("PUT", &f.path(), &f.owner, Body::from(old))?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    let reached = Arc::new(tokio::sync::Notify::new());
    let signal = Arc::clone(&reached);
    let body = Body::from_stream(
        futures_util::stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(b"unverified replacement"))
        })
        .chain(futures_util::stream::once(async move {
            signal.notify_one();
            std::future::pending::<std::result::Result<Bytes, std::io::Error>>().await
        })),
    );
    {
        let upload = f.a.raw_response(request("PUT", &f.path(), &f.owner, body)?);
        tokio::pin!(upload);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                () = reached.notified() => Ok(()),
                result = &mut upload => Err(format!("replacement ended before body pause: {:?}", result.map(|r|r.status()))),
            }
        }).await??;
        let response =
            f.b.request(request("GET", &f.path(), &f.owner, Body::empty())?)
                .await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(response.body(), old.as_bytes());
    }
    f.assert_staging_cleaned().await?;
    let response =
        f.b.request(request("GET", &f.path(), &f.owner, Body::empty())?)
            .await?;
    response.assert_status(StatusCode::OK)?;
    assert_eq!(response.body(), old.as_bytes());
    f.close().await
}
