//! 真实 HTTP 租户成员授权；Redis/对象与数据库 worker 的隔离另需对应 feature 与服务。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
use super::models::tenant_membership;
use figment::{Figment, providers::Serialized};
#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
use futures_util::FutureExt as _;
use openrust::Db;
use openrust::auth::{AuthFlows, JwtTokens};
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum;
use openrust::http::axum::http::StatusCode;
use openrust::testing::{TestApp, TestDatabase};
#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
use std::fmt::Write as _;
use std::time::Duration;
use tokio::sync::oneshot;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const BUDGET: Duration = Duration::from_secs(10);
const ROOT: &str = "/api/example/tenant/record";
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Server {
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Server {
    async fn close(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            if let Ok(result) = tokio::time::timeout(BUDGET, &mut task).await {
                result??;
            } else {
                task.abort();
                let _ = tokio::time::timeout(BUDGET, task).await;
                return Err("tenant server shutdown exceeded deadline".into());
            }
        }
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    _directory: tempfile::TempDir,
    server: Server,
    client: reqwest::Client,
    origin: String,
    owner: i64,
    token: String,
    other: String,
    #[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
    redis_store: bool,
}
impl Fixture {
    async fn start(store: serde_json::Value) -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping real tenant module test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let directory = tempfile::tempdir()?;
        #[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
        let redis_store = store.get("driver").and_then(serde_json::Value::as_str) == Some("redis");
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let mut framework = openrust::config::FrameworkConfig::default();
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let mut configuration = serde_json::json!({
            "app":{"env":"test"},"database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":store}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"tenant-example","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        });
        #[cfg(feature = "storage-local")]
        {
            configuration["storage"] = serde_json::json!({"default_disk":"local","disks":{"local":{"driver":"fs","root":directory.path()}}});
        }
        let source = Figment::from(Serialized::defaults(framework))
            .merge(Serialized::defaults(configuration.take()));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        let auth = app.app()?.state().resolve::<AuthFlows>().await?;
        let tokens = app.app()?.state().resolve::<JwtTokens>().await?;
        let mut identities = Vec::new();
        for email in ["tenant-owner@example.test", "tenant-other@example.test"] {
            let password = "isolated tenant fixture password";
            let code = auth
                .register(email, password, "tenant-test")
                .await?
                .ok_or("verification code missing")?;
            assert!(auth.verify_email(email, code.code(), "tenant-test").await?);
            let id = auth.login(email, password, "tenant-test").await?.user_id();
            identities.push((id, tokens.issue_access_token(id)?));
        }
        let owner = identities[0].0;
        let db = app.app()?.state().resolve::<Db>().await?;
        for (tenant, user) in [
            ("tenant-a", owner),
            ("tenant-b", owner),
            ("tenant-a", identities[1].0),
        ] {
            tenant_membership::ActiveModel {
                tenant_id: Set(tenant.into()),
                user_id: Set(user),
            }
            .insert(&db.connection()?)
            .await?;
        }
        let router = openrust::http::build_router(app.app()?)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(BUDGET)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let (_, token) = identities.remove(0);
        let (_, other) = identities.remove(0);
        Ok(Some(Self {
            app,
            database,
            _directory: directory,
            server: Server {
                stop: Some(stop),
                task: Some(task),
            },
            client,
            origin,
            owner,
            token,
            other,
            #[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
            redis_store,
        }))
    }
    async fn request(
        &self,
        method: reqwest::Method,
        suffix: &str,
        tenant: Option<&str>,
        token: Option<&str>,
        value: Option<serde_json::Value>,
    ) -> TestResult<reqwest::Response> {
        let mut request = self
            .client
            .request(method, format!("{}{ROOT}{suffix}", self.origin));
        if let Some(tenant) = tenant {
            request = request.header("x-example-tenant", tenant);
        }
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(value) = value {
            request = request.json(&value);
        }
        Ok(request.send().await?)
    }
    async fn put(&self, tenant: &str, value: &str) -> TestResult {
        let response = self
            .request(
                reqwest::Method::PUT,
                "",
                Some(tenant),
                Some(&self.token),
                Some(serde_json::json!({"value":value})),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }
    async fn get(&self, tenant: &str) -> TestResult<serde_json::Value> {
        let response = self
            .request(
                reqwest::Method::GET,
                "",
                Some(tenant),
                Some(&self.token),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        bounded_json(response).await
    }
    #[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
    async fn cleanup_redis(&self) -> TestResult {
        if !self.redis_store {
            return Ok(());
        }
        let cache = self
            .app
            .app()?
            .state()
            .resolve::<openrust::cache::Cache>()
            .await?;
        let key = format!("example:tenant:record:{}", self.owner);
        for tenant in ["tenant-a", "tenant-b", "tenant-c"] {
            openrust::tenant::TenantContext::scope(
                openrust::tenant::TenantId::new(tenant)?,
                async { cache.remove(&key).await },
            )
            .await??;
        }
        // 未恢复 metadata 的任务可能误写全局 key；只清本测试随机 namespace 内这个精确 key。
        cache.remove(&key).await?;
        Ok(())
    }
    async fn close(mut self) -> TestResult {
        let result = self
            .app
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await;
        self.server.close().await?;
        result?;
        self.database.close().await?;
        Ok(())
    }
}
async fn bounded_json(mut response: reqwest::Response) -> TestResult<serde_json::Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 16 * 1024 {
            return Err("tenant response exceeded its budget".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
#[tokio::test]
async fn real_http_requires_membership_and_same_owner_records_remain_tenant_scoped() -> TestResult {
    let Some(fixture) = Fixture::start(serde_json::json!({"driver":"memory"})).await? else {
        return Ok(());
    };
    let anonymous = fixture
        .request(reqwest::Method::GET, "", Some("tenant-a"), None, None)
        .await?;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    for selector in [None, Some("tenant-not-a-member"), Some("../../tenant-a")] {
        let denied = fixture
            .request(
                reqwest::Method::GET,
                "",
                selector,
                Some(&fixture.token),
                None,
            )
            .await?;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
    let duplicated = fixture
        .client
        .get(format!("{}{ROOT}", fixture.origin))
        .bearer_auth(&fixture.token)
        .header("x-example-tenant", "tenant-a")
        .header("x-example-tenant", "tenant-b")
        .send()
        .await?;
    assert_eq!(duplicated.status(), StatusCode::FORBIDDEN);
    fixture.put("tenant-a", "value-a").await?;
    fixture.put("tenant-b", "value-b").await?;
    assert_eq!(fixture.get("tenant-a").await?["data"]["value"], "value-a");
    assert_eq!(fixture.get("tenant-b").await?["data"]["value"], "value-b");
    let foreign_owner = fixture
        .request(
            reqwest::Method::GET,
            "",
            Some("tenant-a"),
            Some(&fixture.other),
            None,
        )
        .await?;
    assert_eq!(foreign_owner.status(), StatusCode::NOT_FOUND);
    // 客户端不能用 JSON 偷换持久化 owner 或 tenant；双约束来自身份和可信上下文。
    let forged = fixture
        .request(
            reqwest::Method::PUT,
            "",
            Some("tenant-a"),
            Some(&fixture.token),
            Some(serde_json::json!({"value":"forged","tenant_id":"tenant-b","owner_id":123})),
        )
        .await?;
    assert!(!forged.status().is_success());
    fixture.put("tenant-b", "new-b").await?;
    assert_eq!(fixture.get("tenant-a").await?["data"]["value"], "value-a");
    assert_eq!(fixture.get("tenant-b").await?["data"]["value"], "new-b");
    // 缓存命中不能越过主库成员检查。
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    tenant_membership::Entity::delete_by_id(("tenant-a".to_owned(), fixture.owner))
        .exec(&db.connection()?)
        .await?;
    let revoked = fixture
        .request(
            reqwest::Method::GET,
            "",
            Some("tenant-a"),
            Some(&fixture.token),
            None,
        )
        .await?;
    assert_eq!(revoked.status(), StatusCode::FORBIDDEN);
    assert_eq!(fixture.get("tenant-b").await?["data"]["value"], "new-b");
    drop(db);
    fixture.close().await
}

#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn real_redis_and_storage_follow_persisted_tenant_metadata_in_database_worker() -> TestResult
{
    use super::services::tenant::TENANT_QUEUE;
    use openrust::queue::Queue;
    use openrust::tenant::TenantContext;
    let redis = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce)?;
    let namespace = format!(
        "example-tenant-{}",
        nonce.iter().fold(String::new(), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
    );
    let Some(fixture) =
        Fixture::start(serde_json::json!({"driver":"redis","dsn":redis,"namespace":namespace}))
            .await?
    else {
        return Ok(());
    };
    let result = std::panic::AssertUnwindSafe(async {
        fixture.put("tenant-a", "redis-a").await?;
        fixture.put("tenant-b", "redis-b").await?;
        let queue = fixture.app.app()?.state().resolve::<Queue>().await?;
        for tenant in ["tenant-a", "tenant-b"] {
            let dispatched = fixture
                .request(
                    reqwest::Method::POST,
                    "/rebuild",
                    Some(tenant),
                    Some(&fixture.token),
                    None,
                )
                .await?;
            assert_eq!(dispatched.status(), StatusCode::OK);
            assert!(TenantContext::current().is_none());
            assert!(
                tokio::time::timeout(
                    BUDGET,
                    queue.run_next_with_state(TENANT_QUEUE, fixture.app.app()?.state())
                )
                .await??
            );
            assert!(
                TenantContext::current().is_none(),
                "worker must not leak the previous attempt tenant"
            );
            let record = fixture.get(tenant).await?;
            assert_eq!(record["data"]["cache_hit"], true);
            assert_eq!(
                record["data"]["value"],
                if tenant == "tenant-a" {
                    "redis-a"
                } else {
                    "redis-b"
                }
            );
        }
        for (tenant, value) in [("tenant-a", "redis-a"), ("tenant-b", "redis-b")] {
            let file = fixture
                .request(
                    reqwest::Method::GET,
                    "/file",
                    Some(tenant),
                    Some(&fixture.token),
                    None,
                )
                .await?;
            let file_status = file.status();
            if file_status != StatusCode::OK {
                let code = bounded_json(file).await?["code"].clone();
                return Err(format!(
                    "tenant file read failed with status {file_status} and code {code}"
                )
                .into());
            }
            assert_eq!(
                file.headers()
                    .get("cache-control")
                    .and_then(|value| value.to_str().ok()),
                Some("private, no-store")
            );
            // 小型固定服务端值；对象只能来自已授权 tenant/owner 的键。
            assert_eq!(file.text().await?, value);
        }
        let written = fixture
            .request(
                reqwest::Method::PUT,
                "/file",
                Some("tenant-a"),
                Some(&fixture.token),
                None,
            )
            .await?;
        let written_status = written.status();
        let written_code = bounded_json(written).await?["code"].clone();
        assert_eq!(written_status, StatusCode::OK, "{written_code}");
        let denied = fixture
            .request(
                reqwest::Method::GET,
                "/file",
                Some("tenant-b"),
                Some(&fixture.other),
                None,
            )
            .await?;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        // 独立拒绝路径：直接调用 Job 时只有 task-local 而无 worker metadata 也不能冒充可信尝试。
        {
            use openrust::queue::{Job, JobContext};
            let context = JobContext::new(
                "tenant-no-metadata",
                1,
                1,
                openrust::core::Cancellation::new(),
            )?
            .with_app_state(fixture.app.app()?.state().clone());
            let denied =
                TenantContext::scope(openrust::tenant::TenantId::new("tenant-a")?, async {
                    let job = super::services::tenant::TenantRecordJob {
                        owner_id: fixture.owner,
                    };
                    job.handle(context).await
                })
                .await?;
            assert!(matches!(denied, Err(openrust::Error::Forbidden(_))));
        }
        let db = fixture.app.app()?.state().resolve::<Db>().await?;
        tenant_membership::Entity::delete_by_id(("tenant-a".to_owned(), fixture.owner))
            .exec(&db.connection()?)
            .await?;
        let revoked = fixture
            .request(
                reqwest::Method::GET,
                "/file",
                Some("tenant-a"),
                Some(&fixture.token),
                None,
            )
            .await?;
        assert_eq!(revoked.status(), StatusCode::FORBIDDEN);
        // 已缓存/已写对象仍在，但撤销主库成员关系后不可读取；对象派生写入为 at-least-once 可重做。
        drop(db);
        drop(queue);
        Ok(())
    })
    .catch_unwind()
    .await;
    finish_redis(fixture, result).await
}

#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
async fn finish_redis(fixture: Fixture, result: std::thread::Result<TestResult>) -> TestResult {
    // 断言 panic 或可预期失败都先精确清理本测试键，再关闭应用；不扫 Redis keyspace。
    let cleanup = tokio::time::timeout(BUDGET, fixture.cleanup_redis()).await;
    let closed = fixture.close().await;
    let cleanup_failed = !matches!(cleanup, Ok(Ok(())));
    let close_failed = closed.is_err();
    if cleanup_failed {
        eprintln!("tenant test fixture cache cleanup failed");
    }
    if close_failed {
        eprintln!("tenant test fixture app shutdown failed");
    }
    match result {
        Ok(Err(error)) => Err(error),
        Err(_) => Err("tenant regression assertion failed after cleanup".into()),
        Ok(Ok(())) if cleanup_failed => Err("tenant fixture cache cleanup failed".into()),
        Ok(Ok(())) if close_failed => Err("tenant fixture app shutdown failed".into()),
        Ok(Ok(())) => Ok(()),
    }
}

#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
#[derive(Clone, Copy)]
enum BrokenMessage {
    MissingMetadata,
    OtherTenant,
    RevokedMember,
}

#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
#[tokio::test]
async fn persisted_worker_rejects_missing_unentitled_or_revoked_tenant_before_side_effects()
-> TestResult {
    for broken in [
        BrokenMessage::MissingMetadata,
        BrokenMessage::OtherTenant,
        BrokenMessage::RevokedMember,
    ] {
        let redis = std::env::var("OPENRUST_TEST_REDIS_URL")
            .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce)?;
        let namespace = format!(
            "example-tenant-failure-{}",
            nonce.iter().fold(String::new(), |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            })
        );
        let Some(fixture) =
            Fixture::start(serde_json::json!({"driver":"redis","dsn":redis,"namespace":namespace}))
                .await?
        else {
            return Ok(());
        };
        let result =
            std::panic::AssertUnwindSafe(async { run_broken_message(&fixture, broken).await })
                .catch_unwind()
                .await;
        finish_redis(fixture, result).await?;
    }
    Ok(())
}

#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn run_broken_message(fixture: &Fixture, broken: BrokenMessage) -> TestResult {
    use super::models::tenant_record;
    use super::services::tenant::{TENANT_QUEUE, TenantRecordJob};
    use openrust::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
    use openrust::queue::{Job, Queue};
    fixture.put("tenant-a", "allowed-a").await?;
    fixture.put("tenant-b", "allowed-b").await?;
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let connection = db.primary_connection()?;
    // 目标租户确有数据，但当前 owner 没有成员资格；这不等于可防完整 DB 写权限者伪造全部授权。
    tenant_record::ActiveModel {
        tenant_id: Set("tenant-c".into()),
        owner_id: Set(fixture.owner),
        value: Set("unentitled-c".into()),
    }
    .insert(&connection)
    .await?;
    let before = tenant_record::Entity::find().all(&connection).await?;
    assert_no_projection(fixture).await?;
    let response = fixture
        .request(
            reqwest::Method::POST,
            "/rebuild",
            Some("tenant-a"),
            Some(&fixture.token),
            None,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = bounded_json(response).await?;
    let id = body["data"]["job_id"]
        .as_str()
        .ok_or("persisted tenant job ID missing")?
        .to_owned();
    let backend = db.backend()?;
    let jobs = db.quoted_table_name("jobs")?;
    let parameter = if backend == DbBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!("SELECT payload,job_name,queue FROM {jobs} WHERE id = {parameter}"),
            [id.clone().into()],
        ))
        .await?
        .ok_or("isolated dispatched job missing")?;
    assert_eq!(
        row.try_get::<String>("", "job_name")?,
        TenantRecordJob::NAME
    );
    assert_eq!(row.try_get::<String>("", "queue")?, TENANT_QUEUE);
    let mut payload: serde_json::Value = row.try_get("", "payload")?;
    assert_eq!(payload["metadata"]["tenant_id"], "tenant-a");
    match broken {
        BrokenMessage::MissingMetadata => {
            payload = payload
                .get("payload")
                .cloned()
                .ok_or("queue execution envelope missing")?;
        }
        BrokenMessage::OtherTenant => {
            payload["metadata"]["tenant_id"] = serde_json::json!("tenant-c");
        }
        BrokenMessage::RevokedMember => {
            tenant_membership::Entity::delete_by_id(("tenant-a".to_owned(), fixture.owner))
                .exec(&connection)
                .await?;
        }
    }
    let update = match backend {
        DbBackend::Postgres => format!(
            "UPDATE {jobs} SET payload = $1::jsonb,max_attempts = 1 WHERE id = $2 AND job_name = $3 AND queue = $4"
        ),
        DbBackend::MySql => format!(
            "UPDATE {jobs} SET payload = CAST(? AS JSON),max_attempts = 1 WHERE id = ? AND job_name = ? AND queue = ?"
        ),
        _ => return Err("unsupported tenant test database".into()),
    };
    // 只修改本次 HTTP 派发返回的精确任务行；不更新任何共享队列或其他 ID。
    assert_eq!(
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                update,
                [
                    serde_json::to_string(&payload)?.into(),
                    id.clone().into(),
                    TenantRecordJob::NAME.into(),
                    TENANT_QUEUE.into()
                ]
            ))
            .await?
            .rows_affected(),
        1
    );
    let queue = fixture.app.app()?.state().resolve::<Queue>().await?;
    assert!(
        tokio::time::timeout(
            BUDGET,
            queue.run_next_with_state(TENANT_QUEUE, fixture.app.app()?.state())
        )
        .await??
    );
    // 已领取不等于成功：以持久 failed_jobs 及活跃行移除证明失败状态，而非只看 worker bool。
    let failed = db.quoted_table_name("failed_jobs")?;
    let failed = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!("SELECT error FROM {failed} WHERE id = {parameter}"),
            [id.clone().into()],
        ))
        .await?
        .ok_or("rejected tenant job not persisted as failed")?;
    let error: String = failed.try_get("", "error")?;
    assert_eq!(
        error,
        match broken {
            BrokenMessage::MissingMetadata => "tenant_context_missing",
            _ => "forbidden",
        }
    );
    assert!(
        connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!("SELECT id FROM {jobs} WHERE id = {parameter}"),
                [id.into()]
            ))
            .await?
            .is_none()
    );
    assert_no_projection(fixture).await?;
    let after = tenant_record::Entity::find().all(&connection).await?;
    assert_eq!(before.len(), after.len());
    for record in before {
        assert!(after.contains(&record));
    }
    // 撤销发生在 consume 之前；不承诺已通过授权并开始的外部 IO 能被瞬间撤回。
    assert!(openrust::tenant::TenantContext::current().is_none());
    Ok(())
}

#[cfg(all(feature = "cache-redis", feature = "queue", feature = "storage-local"))]
async fn assert_no_projection(fixture: &Fixture) -> TestResult {
    use openrust::cache::Cache;
    use openrust::storage::{Storage, StoragePath};
    use openrust::tenant::{TenantContext, TenantId};
    let cache = fixture.app.app()?.state().resolve::<Cache>().await?;
    let disk = fixture
        .app
        .app()?
        .state()
        .resolve::<Storage>()
        .await?
        .default_disk()
        .await?;
    let key = format!("example:tenant:record:{}", fixture.owner);
    let path = StoragePath::new(format!("example/tenant-record/{}.txt", fixture.owner))?;
    for tenant in ["tenant-a", "tenant-b", "tenant-c"] {
        TenantContext::scope(TenantId::new(tenant)?, async {
            assert!(cache.get::<String>(&key).await?.is_none());
            let error =
                disk.stat(&path).await.err().ok_or_else(|| {
                    openrust::Error::BadRequest("unexpected tenant object".into())
                })?;
            assert_eq!(error.code(), "storage_object_not_found");
            Ok::<(), openrust::Error>(())
        })
        .await??;
    }
    // 丢失上下文不得写入未分区的全局派生对象。
    assert!(cache.get::<String>(&key).await?.is_none());
    let error = disk
        .stat(&path)
        .await
        .err()
        .ok_or("unexpected unscoped object")?;
    assert_eq!(error.code(), "storage_object_not_found");
    Ok(())
}
