//! 真实 S3/Redis、双独立应用与数据库 worker 的租户分区；不代表多进程集群。
#![cfg(all(
    feature = "auth",
    feature = "storage-s3",
    feature = "storage-local",
    feature = "queue",
    feature = "cache-redis",
    any(feature = "db-postgres", feature = "db-mysql")
))]
use super::http::requests::attachments::AttachmentUpload;
use super::models::{article, attachment_cleanup, tenant_membership};
use super::s3_flows::{Fixture, HttpServer};
use super::services::{attachments::ArticleAttachments, tenant::TENANT_QUEUE};
use futures_util::{FutureExt, StreamExt};
use openrust::Db;
use openrust::cache::Cache;
use openrust::database::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, EntityTrait, TransactionTrait,
};
use openrust::filesystem::FilesystemManager;
use openrust::filesystem::StoragePath;
use openrust::filesystem::StorageReader;
use openrust::filesystem::UploadOptions;
use openrust::http::axum::{body::Body, http::StatusCode};
use openrust::queue::Queue;
use openrust::tenant::{TenantContext, TenantId};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);
const A: &str = "tenant-s3-a";
const B: &str = "tenant-s3-b";
const RECORD: &str = "/api/example/tenant/record";
const CONTROL: &str = "control/must-remain.txt";
const CACHE_CONTROL: &str = "control:must-remain";
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore = "需显式配置真实 MinIO OPENRUST_TEST_S3_*、Redis 与隔离 PostgreSQL/MySQL"]
async fn s3_tenant_partitions_cross_apps_and_maintenance_resumes_only_recorded_tenant() -> TestResult
{
    let redis = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let namespace = format!("r1e-tenant-s3-{}", uuid::Uuid::now_v7());
    // 复用现有真实 S3 fixture：其 bucket root 亦为独有 UUIDv7，绝不清空共享 bucket。
    let fixture = Fixture::create_with_store(
        serde_json::json!({"driver":"redis","dsn":redis,"namespace":namespace}),
    )
    .await?;
    let a = HttpServer::start(&fixture.a).await?;
    let b = HttpServer::start(&fixture.b).await?;
    let result =
        std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(90), async {
            run_checks(&fixture, &a, &b).await
        }))
        .catch_unwind()
        .await;
    // 无论预期错误、测试断言失败或总期限取消，都精确清自己命名空间中的已知对象与键。
    let require_control = matches!(&result, Ok(Ok(Ok(()))));
    let cleanup = tokio::time::timeout(WAIT, cleanup_known(&fixture, require_control)).await;
    let stopped_a = a.shutdown().await;
    let stopped_b = b.shutdown().await;
    let closed = tokio::time::timeout(WAIT, fixture.close()).await;
    let cleanup_failed = !matches!(cleanup, Ok(Ok(())));
    let stop_failed = stopped_a.is_err() || stopped_b.is_err();
    let close_failed = !matches!(closed, Ok(Ok(())));
    if cleanup_failed || stop_failed || close_failed {
        eprintln!("tenant S3 fixture cleanup or shutdown failed");
    }
    match result {
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) => return Err("tenant S3 checks exceeded their budget".into()),
        Err(_) => return Err("tenant S3 assertion failed after fixture cleanup".into()),
        Ok(Ok(Ok(()))) => {}
    }
    if cleanup_failed || stop_failed || close_failed {
        return Err("tenant S3 fixture cleanup or shutdown failed".into());
    }
    Ok(())
}
#[allow(clippy::too_many_lines)] // 双应用真实 S3 租户隔离、取消与清理是同一验收闭环。
async fn run_checks(fixture: &Fixture, a: &HttpServer, b: &HttpServer) -> TestResult {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(WAIT)
        .build()?;
    for app in [&fixture.a, &fixture.b] {
        let config = app
            .app()?
            .section::<openrust::config::StorageConfig>("storage")?;
        assert_eq!(config.default_disk, "shared");
        assert_eq!(
            config
                .disks
                .get("shared")
                .ok_or("shared disk missing")?
                .driver,
            "s3"
        );
    }
    let db = fixture.a.app()?.state().resolve::<Db>().await?;
    let connection = db.primary_connection()?;
    for tenant in [A, B] {
        tenant_membership::ActiveModel {
            tenant_id: Set(tenant.into()),
            user_id: Set(1),
        }
        .insert(&connection)
        .await?;
    }
    let disk = fixture
        .a
        .app()?
        .state()
        .resolve::<FilesystemManager>()
        .await?
        .default_disk()
        .await?;
    disk.put_body(
        StoragePath::new(CONTROL)?,
        Some("text/plain".into()),
        Body::from("control outside business keys"),
        UploadOptions::default(),
        None,
    )
    .await?;
    fixture
        .a
        .app()?
        .state()
        .resolve::<Cache>()
        .await?
        .set(
            CACHE_CONTROL,
            &"preserve-control",
            Some(Duration::from_secs(120)),
        )
        .await?;
    for (tenant, value, writer) in [(A, "S3 tenant A", a), (B, "S3 tenant B", b)] {
        let response = client
            .put(format!("{}{RECORD}", writer.origin))
            .bearer_auth(&fixture.owner)
            .header("x-example-tenant", tenant)
            .json(&serde_json::json!({"value":value}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let response = client
            .post(format!("{}{RECORD}/rebuild", writer.origin))
            .bearer_auth(&fixture.owner)
            .header("x-example-tenant", tenant)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(TenantContext::current().is_none());
        // App B 领取真实持久队列；不共享 App A 的 Cache/Storage/Container 内存对象。
        let queue = fixture.b.app()?.state().resolve::<Queue>().await?;
        assert!(
            tokio::time::timeout(
                WAIT,
                queue.run_next_with_state(TENANT_QUEUE, fixture.b.app()?.state())
            )
            .await??
        );
        assert!(TenantContext::current().is_none());
        for reader in [a, b] {
            let response = client
                .get(format!("{}{RECORD}", reader.origin))
                .bearer_auth(&fixture.owner)
                .header("x-example-tenant", tenant)
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
            let data = bounded_body(response).await?;
            let data: serde_json::Value = serde_json::from_slice(&data)?;
            assert_eq!(data["data"]["value"], value);
            assert_eq!(data["data"]["cache_hit"], true);
            let file = client
                .get(format!("{}{RECORD}/file", reader.origin))
                .bearer_auth(&fixture.owner)
                .header("x-example-tenant", tenant)
                .send()
                .await?;
            assert_eq!(file.status(), StatusCode::OK);
            assert_eq!(bounded_body(file).await?, value.as_bytes());
        }
    }
    // 直接观察自有 bucket prefix 内实际物理对象：同一逻辑 owner key 对应两条独立租户路径。
    let objects = fixture
        .observer
        .list_with("tenants/")
        .recursive(true)
        .await?;
    let records = objects
        .iter()
        .filter(|entry| entry.path().ends_with("/example/tenant-record/1.txt"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert_ne!(records[0].path(), records[1].path());
    let mut values = Vec::new();
    for record in records {
        values.push(fixture.observer.read(record.path()).await?.to_vec());
    }
    assert!(values.contains(&b"S3 tenant A".to_vec()));
    assert!(values.contains(&b"S3 tenant B".to_vec()));

    let attachments_a = fixture
        .a
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let attachments_b = fixture
        .b
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    // 附件路由本身未增加租户选择器；这里使用可信 TenantContext 调用公开模块业务 API，非伪造 HTTP 授权。
    for (tenant, value) in [(A, "attachment A"), (B, "attachment B remains")] {
        TenantContext::scope(TenantId::new(tenant)?, async {
            attachments_a
                .upload(
                    1,
                    fixture.id,
                    AttachmentUpload::from_content_type("text/plain")?,
                    Body::from(value),
                )
                .await
        })
        .await??;
        let opened =
            TenantContext::scope(TenantId::new(tenant)?, attachments_b.open(1, fixture.id))
                .await??;
        assert_eq!(read_object(opened).await?, value.as_bytes());
    }
    let transaction = connection.begin().await?;
    article::Entity::delete_by_id(fixture.id)
        .exec(&transaction)
        .await?;
    TenantContext::scope(
        TenantId::new(A)?,
        attachments_a.record_deleted(&transaction, 1, fixture.id),
    )
    .await??;
    transaction.commit().await?;
    let intent = attachment_cleanup::Entity::find_by_id(fixture.id)
        .one(&connection)
        .await?
        .ok_or("attachment cleanup intent missing")?;
    assert_eq!(intent.tenant_id.as_deref(), Some(A));
    let rejected = TenantContext::scope(
        TenantId::new(B)?,
        attachments_b.cleanup_deleted(1, fixture.id),
    )
    .await?
    .err()
    .ok_or("foreign ambient cleanup was accepted")?;
    assert_eq!(
        rejected.code(),
        "example_attachment_cleanup_tenant_mismatch"
    );
    let rejected = TenantContext::scope(TenantId::new(B)?, attachments_b.reconcile_pending())
        .await?
        .err()
        .ok_or("foreign ambient maintenance was accepted")?;
    assert_eq!(
        rejected.code(),
        "example_attachment_cleanup_tenant_mismatch"
    );
    assert!(
        attachment_cleanup::Entity::find_by_id(fixture.id)
            .one(&connection)
            .await?
            .is_some()
    );
    let object = fixture.object()?;
    for tenant in [A, B] {
        assert!(
            TenantContext::scope(TenantId::new(tenant)?, disk.stat(&object))
                .await??
                .size
                > 0
        );
    }
    // 无 ambient 的维护从持久意图恢复 A，而不是从当前 task 猜租户。
    assert!(TenantContext::current().is_none());
    assert_eq!(attachments_b.reconcile_pending().await?, 1);
    assert!(
        attachment_cleanup::Entity::find_by_id(fixture.id)
            .one(&connection)
            .await?
            .is_none()
    );
    let absent = TenantContext::scope(TenantId::new(A)?, disk.stat(&object))
        .await?
        .err()
        .ok_or("tenant A object was not removed")?;
    assert_eq!(absent.code(), "storage_object_not_found");
    let remaining = TenantContext::scope(TenantId::new(B)?, disk.open(&object, None)).await??;
    assert_eq!(read_object(remaining).await?, b"attachment B remains");
    assert!(disk.stat(&StoragePath::new(CONTROL)?).await?.size > 0);
    Ok(())
}
async fn bounded_body(mut response: reqwest::Response) -> TestResult<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > 16 * 1024 {
            return Err("tenant S3 response exceeded budget".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
async fn read_object(mut reader: StorageReader) -> TestResult<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = reader.body.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > 16 * 1024 {
            return Err("tenant S3 object exceeded budget".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
async fn cleanup_known(fixture: &Fixture, require_control: bool) -> TestResult {
    let storage = fixture
        .b
        .app()?
        .state()
        .resolve::<FilesystemManager>()
        .await?;
    let disk = storage.default_disk().await?;
    let cache = fixture.b.app()?.state().resolve::<Cache>().await?;
    let cache_key = "example:tenant:record:1";
    let record = StoragePath::new("example/tenant-record/1.txt")?;
    let attachment = fixture.object()?;
    for tenant in [A, B] {
        TenantContext::scope(TenantId::new(tenant)?, async {
            disk.delete(&record).await?;
            disk.delete(&attachment).await?;
            cache.remove(cache_key).await
        })
        .await??;
    }
    // 精确清理潜在误写的未分区 key；namespace/root 仍是本 fixture 独有的随机前缀。
    cache.remove(cache_key).await?;
    disk.delete(&record).await?;
    disk.delete(&attachment).await?;
    let control = StoragePath::new(CONTROL)?;
    // 若测试已写入控制对象，业务键清理必须保留它；最后显式删除自己的控制对象。
    match disk.stat(&control).await {
        Ok(metadata) => {
            assert!(metadata.size > 0);
            disk.delete(&control).await?;
        }
        Err(error) if error.code() == "storage_object_not_found" && !require_control => {}
        Err(error) => return Err(error.into()),
    }
    let control_value = cache.get::<String>(CACHE_CONTROL).await?;
    cache.remove(CACHE_CONTROL).await?;
    if require_control {
        assert_eq!(control_value.as_deref(), Some("preserve-control"));
    }
    Ok(())
}
