//! 文章附件的真实数据库与本地文件流边界；HTTP 为生产 Router 的进程内请求。
#![cfg(all(
    feature = "auth",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use std::sync::Arc;
use std::time::Duration;

use figment::{Figment, providers::Serialized};
use futures_util::{FutureExt, StreamExt};
use openrust::auth::JwtTokens;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, TransactionTrait};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use openrust::storage::{Storage, StoragePath, UploadContext, UploadInspector, UploadOptions};
use openrust::tenant::{TenantContext, TenantId};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Db, Error, Result};

use super::http::requests::attachments::{AttachmentUpload, MAX_ATTACHMENT_BYTES};
use super::models::{article, attachment_cleanup};
use super::services::attachments::{ArticleAttachments, TextAttachmentInspector};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct AttachmentMigrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for AttachmentMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    directory: tempfile::TempDir,
    article_id: i64,
    owner_token: String,
    other_token: String,
}
impl Fixture {
    async fn create() -> std::result::Result<Option<Self>, Box<dyn std::error::Error>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping article attachment DB test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<AttachmentMigrator>(&admin).await?;
        let directory = tempfile::tempdir()?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source = Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(
            serde_json::json!({
                "app":{"env":"test"},
                "database":{"url":database.url()},
                "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
                "auth":{"jwt":{"issuer":"http://example.test","audience":"attachment-tests","active_key_id":"test","keys":{"test":key}}},
                "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
                "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":directory.path()}}}
            }),
        ));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let model = article::ActiveModel {
            owner_id: Set(1),
            title: Set("Attachment test".into()),
            body: Set("A private note".into()),
            ..Default::default()
        }
        .insert(&db.connection()?)
        .await?;
        let tokens = app.app()?.state().resolve::<JwtTokens>().await?;
        let owner_token = tokens.issue_access_token(1)?;
        let other_token = tokens.issue_access_token(2)?;
        Ok(Some(Self {
            app,
            database,
            directory,
            article_id: model.id,
            owner_token,
            other_token,
        }))
    }
    fn path(&self) -> String {
        format!("/api/example/articles/{}/attachment", self.article_id)
    }
    fn object(&self) -> std::path::PathBuf {
        self.directory.path().join(format!(
            "example/articles/1/{}/attachment.txt",
            self.article_id
        ))
    }
    async fn close(mut self) -> TestResult {
        let shutdown = self
            .app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await;
        let database = self.database.close().await;
        shutdown?;
        database?;
        Ok(())
    }
}
fn request(
    method: &str,
    path: &str,
    token: Option<&str>,
    mime: &str,
    body: Body,
) -> Result<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", mime)
        .header(
            "content-disposition",
            "attachment; filename=\"../../untrusted-name\"",
        );
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(body).map_err(Error::internal)
}

#[tokio::test]
async fn multipart_attachment_uses_public_file_extractor_and_existing_owner_guard() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let result = std::panic::AssertUnwindSafe(async {
        let body = concat!(
            "--example-file\r\nContent-Disposition: form-data; name=\"document\"; filename=\"../../client.txt\"\r\nContent-Type: text/plain\r\n\r\npublic Request file\r\n",
            "--example-file--\r\n"
        );
        let path = format!("{}/file", fixture.path());
        for (token, status) in [
            (&fixture.other_token, StatusCode::NOT_FOUND),
            (&fixture.owner_token, StatusCode::CREATED),
        ] {
            fixture
                .app
                .request(request(
                    "POST",
                    &path,
                    Some(token),
                    "multipart/form-data; boundary=example-file",
                    Body::from(body),
                )?)
                .await?
                .assert_status(status)?;
        }
        let declared_binary = body.replace("Content-Type: text/plain", "Content-Type: application/octet-stream");
        fixture
            .app
            .request(request(
                "POST", &path, Some(&fixture.owner_token),
                "multipart/form-data; boundary=example-file", Body::from(declared_binary),
            )?)
            .await?
            .assert_status(StatusCode::CREATED)?;
        let disguised_pdf = body.replace(
            "public Request file",
            "%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF\n",
        );
        fixture
            .app
            .request(request(
                "POST", &path, Some(&fixture.owner_token),
                "multipart/form-data; boundary=example-file", Body::from(disguised_pdf),
            )?)
            .await?
            .assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE)?;
        assert_eq!(
            tokio::fs::read(fixture.object()).await?,
            b"public Request file"
        );
        assert!(!fixture.directory.path().join("client.txt").exists());
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .catch_unwind()
    .await;
    let closed = std::panic::AssertUnwindSafe(fixture.close())
        .catch_unwind()
        .await;
    match result {
        Ok(Ok(())) => match closed {
            Ok(cleanup) => cleanup,
            Err(payload) => std::panic::resume_unwind(payload),
        },
        Ok(Err(error)) => {
            if !matches!(closed, Ok(Ok(()))) {
                eprintln!("REQUEST_FILE_FIXTURE_CLEANUP secondary_failure");
            }
            Err(error)
        }
        Err(payload) => {
            if !matches!(closed, Ok(Ok(()))) {
                eprintln!("REQUEST_FILE_FIXTURE_CLEANUP secondary_failure");
            }
            std::panic::resume_unwind(payload)
        }
    }
}

#[tokio::test]
async fn attachments_enforce_owner_mime_limits_and_atomic_local_publication() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let path = fixture.path();
    assert_access_rejections(&fixture, &path).await?;
    let text = "A UTF-8 note: 中文🙂\n";
    fixture
        .app
        .request(request(
            "PUT",
            &path,
            Some(&fixture.owner_token),
            "text/plain; charset=utf-8",
            Body::from_stream(futures_util::stream::iter([
                Ok::<_, std::io::Error>(Bytes::copy_from_slice(&text.as_bytes()[..15])),
                Ok(Bytes::copy_from_slice(&text.as_bytes()[15..])),
            ])),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    assert_eq!(tokio::fs::read(fixture.object()).await?, text.as_bytes());
    assert!(!fixture.directory.path().join("untrusted-name").exists());
    for body in [
        Body::from(vec![b'x'; usize::try_from(MAX_ATTACHMENT_BYTES)? + 1]),
        Body::from(vec![0x89, b'P', b'N', b'G']),
        Body::from("bad\0text"),
        Body::empty(),
    ] {
        let response = fixture
            .app
            .request(request(
                "PUT",
                &path,
                Some(&fixture.owner_token),
                "text/plain",
                body,
            )?)
            .await?;
        assert!(matches!(
            response.status(),
            StatusCode::PAYLOAD_TOO_LARGE | StatusCode::UNPROCESSABLE_ENTITY
        ));
        // 失败替换不能损坏已完整发布的旧对象。
        assert_eq!(tokio::fs::read(fixture.object()).await?, text.as_bytes());
    }
    let downloaded = fixture
        .app
        .request(request(
            "GET",
            &path,
            Some(&fixture.owner_token),
            "text/plain",
            Body::empty(),
        )?)
        .await?;
    downloaded
        .assert_status(StatusCode::OK)?
        .assert_header("content-type", "text/plain; charset=utf-8")?
        .assert_header(
            "content-disposition",
            "attachment; filename=\"article-note.txt\"",
        )?
        .assert_header("x-content-type-options", "nosniff")?;
    assert_eq!(downloaded.body(), text.as_bytes());
    fixture
        .app
        .request(request(
            "DELETE",
            &path,
            Some(&fixture.owner_token),
            "text/plain",
            Body::empty(),
        )?)
        .await?
        .assert_status(StatusCode::NO_CONTENT)?;
    assert!(!fixture.object().exists());
    fixture
        .app
        .request(request(
            "GET",
            &path,
            Some(&fixture.owner_token),
            "text/plain",
            Body::empty(),
        )?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    fixture.close().await
}

#[tokio::test]
async fn detached_upload_preserves_tenant_storage_scope() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let storage = fixture.app.app()?.state().resolve::<Storage>().await?;
    let path = StoragePath::new(format!(
        "example/articles/1/{}/attachment.txt",
        fixture.article_id
    ))?;
    let tenant = TenantId::new("attachment-upload-tenant")?;
    TenantContext::scope(tenant.clone(), async {
        attachments
            .upload(
                1,
                fixture.article_id,
                AttachmentUpload::from_content_type("text/plain")?,
                Body::from("tenant-scoped upload"),
            )
            .await
    })
    .await??;

    let disk = storage.default_disk().await?;
    assert_eq!(
        TenantContext::scope(tenant, disk.stat(&path)).await??.size,
        20
    );
    assert_eq!(
        disk.stat(&path)
            .await
            .err()
            .ok_or("tenant object must not exist in the unscoped namespace")?
            .code(),
        "storage_object_not_found"
    );
    fixture.close().await
}

#[tokio::test]
async fn deleting_article_cleans_its_attachment_after_commit() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    fixture
        .app
        .request(request(
            "PUT",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("attachment before article delete"),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    assert!(fixture.object().exists());
    let article_path = format!("/api/example/articles/{}", fixture.article_id);
    fixture
        .app
        .request(request(
            "DELETE",
            &article_path,
            Some(&fixture.owner_token),
            "application/json",
            Body::empty(),
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
    assert!(!fixture.object().exists());
    fixture
        .app
        .request(request(
            "GET",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::empty(),
        )?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    fixture.close().await
}

#[tokio::test]
async fn rolled_back_article_delete_keeps_attachment_and_discards_cleanup_intent() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    fixture
        .app
        .request(request(
            "PUT",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("retained after rollback"),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let connection = db.primary_connection()?;
    let transaction = connection.begin().await?;
    article::Entity::delete_by_id(fixture.article_id)
        .exec(&transaction)
        .await?;
    attachments
        .record_deleted(&transaction, 1, fixture.article_id)
        .await?;
    transaction.rollback().await?;
    assert!(fixture.object().exists());
    assert!(
        article::Entity::find_by_id(fixture.article_id)
            .one(&connection)
            .await?
            .is_some()
    );
    assert!(
        attachment_cleanup::Entity::find_by_id(fixture.article_id)
            .one(&connection)
            .await?
            .is_none()
    );
    // 即便同一 ID 再次对应合法文章，遗留意图也不得误删新文章的对象。
    let transaction = connection.begin().await?;
    attachments
        .record_deleted(&transaction, 1, fixture.article_id)
        .await?;
    transaction.commit().await?;
    assert_eq!(
        attachments
            .cleanup_deleted(1, fixture.article_id)
            .await
            .err()
            .ok_or("expected cleanup conflict")?
            .code(),
        "example_attachment_cleanup_conflict"
    );
    assert!(fixture.object().exists());
    assert_eq!(
        attachments
            .upload(
                1,
                fixture.article_id,
                AttachmentUpload::from_content_type("text/plain")?,
                Body::from("would replace newer object"),
            )
            .await
            .err()
            .ok_or("expected pending cleanup to block replacement")?
            .code(),
        "example_attachment_cleanup_conflict"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.object())?,
        "retained after rollback"
    );
    attachment_cleanup::Entity::delete_by_id(fixture.article_id)
        .exec(&connection)
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn cancelled_after_delete_commit_recovers_cleanup_after_app_reset() -> TestResult {
    let Some(mut fixture) = Fixture::create().await? else {
        return Ok(());
    };
    fixture
        .app
        .request(request(
            "PUT",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("committed object"),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let reached = Arc::new(tokio::sync::Notify::new());
    attachments.pause_before_cleanup(
        Arc::clone(&reached),
        Arc::new(tokio::sync::Semaphore::new(0)),
    )?;
    // 使用真实 Router 请求，并在提交后的持久清理入口精确暂停。
    {
        let deletion = fixture.app.raw_response(request(
            "DELETE",
            &format!("/api/example/articles/{}", fixture.article_id),
            Some(&fixture.owner_token),
            "application/json",
            Body::empty(),
        )?);
        tokio::pin!(deletion);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                () = reached.notified() => Ok(()),
                result = &mut deletion => Err(format!("delete finished before cleanup pause: {:?}", result.map(|response| response.status()))),
            }
        })
        .await??;
    }
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let connection = db.primary_connection()?;
    assert!(
        article::Entity::find_by_id(fixture.article_id)
            .one(&connection)
            .await?
            .is_none()
    );
    assert!(fixture.object().exists());
    assert!(
        attachment_cleanup::Entity::find_by_id(fixture.article_id)
            .one(&connection)
            .await?
            .is_some()
    );
    fixture
        .app
        .reset(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    fixture
        .app
        .app()?
        .run_command("example", vec!["attachments".into(), "reconcile".into()])
        .await?;
    assert!(!fixture.object().exists());
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    assert!(
        attachment_cleanup::Entity::find_by_id(fixture.article_id)
            .one(&db.primary_connection()?)
            .await?
            .is_none()
    );
    fixture
        .app
        .app()?
        .run_command("example", vec!["attachments".into(), "reconcile".into()])
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn concurrent_cleanup_cannot_delete_reused_article_attachment() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    fixture
        .app
        .request(request(
            "PUT",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("old article object"),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let connection = db.primary_connection()?;
    let transaction = connection.begin().await?;
    article::Entity::delete_by_id(fixture.article_id)
        .exec(&transaction)
        .await?;
    attachments
        .record_deleted(&transaction, 1, fixture.article_id)
        .await?;
    transaction.commit().await?;
    let first_reached = Arc::new(tokio::sync::Notify::new());
    let first_release = Arc::new(tokio::sync::Semaphore::new(0));
    let second_reached = Arc::new(tokio::sync::Notify::new());
    let second_release = Arc::new(tokio::sync::Semaphore::new(0));
    attachments.pause_after_cleanup_lock(Arc::clone(&first_reached), Arc::clone(&first_release))?;
    attachments
        .pause_after_cleanup_lock(Arc::clone(&second_reached), Arc::clone(&second_release))?;
    let id = fixture.article_id;
    let first_service = Arc::clone(&attachments);
    let first = tokio::spawn(async move { first_service.cleanup_deleted(1, id).await });
    tokio::time::timeout(Duration::from_secs(5), first_reached.notified()).await?;
    let second_started = Arc::new(tokio::sync::Notify::new());
    attachments.signal_before_intent_lock(Arc::clone(&second_started))?;
    let second_service = Arc::clone(&attachments);
    let second = tokio::spawn(async move { second_service.cleanup_deleted(1, id).await });
    tokio::time::timeout(Duration::from_secs(5), second_started.notified()).await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), second_reached.notified())
            .await
            .is_err()
    );
    first_release.add_permits(1);
    first.await??;
    tokio::time::timeout(Duration::from_secs(5), second_reached.notified()).await?;
    // 等待锁后第二个清理已重新读到“无意图”，不持旧快照执行对象删除。
    second_release.add_permits(1);
    second.await??;
    // 同 key 再合法使用时，旧意图及并发清理均已结束。
    article::ActiveModel {
        id: Set(id),
        owner_id: Set(1),
        title: Set("Reused article".into()),
        body: Set("new row".into()),
        ..Default::default()
    }
    .insert(&connection)
    .await?;
    attachments
        .upload(
            1,
            id,
            AttachmentUpload::from_content_type("text/plain")?,
            Body::from("new article object"),
        )
        .await?;
    assert_eq!(
        std::fs::read_to_string(fixture.object())?,
        "new article object"
    );
    fixture.close().await
}

#[tokio::test]
async fn cancellation_after_object_delete_preserves_retryable_intent() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    fixture
        .app
        .request(request(
            "PUT",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("object before cleanup abort"),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let connection = db.primary_connection()?;
    let transaction = connection.begin().await?;
    article::Entity::delete_by_id(fixture.article_id)
        .exec(&transaction)
        .await?;
    attachments
        .record_deleted(&transaction, 1, fixture.article_id)
        .await?;
    transaction.commit().await?;
    let reached = Arc::new(tokio::sync::Notify::new());
    attachments.pause_after_object_delete(
        Arc::clone(&reached),
        Arc::new(tokio::sync::Semaphore::new(0)),
    )?;
    let service = Arc::clone(&attachments);
    let id = fixture.article_id;
    let cleanup = tokio::spawn(async move { service.cleanup_deleted(1, id).await });
    tokio::time::timeout(Duration::from_secs(5), reached.notified()).await?;
    assert!(!fixture.object().exists());
    cleanup.abort();
    assert!(cleanup.await.is_err());
    assert!(
        attachment_cleanup::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .is_some()
    );
    tokio::time::timeout(Duration::from_secs(5), attachments.reconcile_pending()).await??;
    assert!(
        attachment_cleanup::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .is_none()
    );
    fixture.close().await
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 同一验收场景完整保留持久租户、环境冲突与清理证据。
async fn cleanup_uses_persisted_tenant_and_rejects_ambient_tenant() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let result = std::panic::AssertUnwindSafe(async {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let tenants = fixture.directory.path().join("tenants");
            std::fs::create_dir(&tenants)?;
            std::fs::set_permissions(
                fixture.directory.path(),
                std::fs::Permissions::from_mode(0o755),
            )?;
            std::fs::set_permissions(&tenants, std::fs::Permissions::from_mode(0o755))?;
        }
        let db = fixture.app.app()?.state().resolve::<Db>().await?;
        let attachments = fixture
            .app
            .app()?
            .state()
            .resolve::<ArticleAttachments>()
            .await?;
        let storage = fixture.app.app()?.state().resolve::<Storage>().await?;
        let connection = db.primary_connection()?;
        let id = fixture.article_id;
        let path = StoragePath::new(format!("example/articles/1/{id}/attachment.txt"))?;
        let disk = storage.default_disk().await?;
        let tenant_a = TenantId::new("attachment-tenant-a")?;
        let tenant_b = TenantId::new("attachment-tenant-b")?;
        TenantContext::scope(tenant_a.clone(), async {
            disk.put_body(
                path.clone(),
                Some("text/plain".into()),
                Body::from("tenant A object"),
                UploadOptions::default(),
                None,
            )
            .await
        })
        .await??;
        TenantContext::scope(tenant_b.clone(), async {
            disk.put_body(
                path.clone(),
                Some("text/plain".into()),
                Body::from("tenant B object remains"),
                UploadOptions::default(),
                None,
            )
            .await
        })
        .await??;
        assert_eq!(
            TenantContext::scope(tenant_a.clone(), disk.visibility(&path)).await??,
            openrust::storage::Visibility::Private
        );
        TenantContext::scope(
            tenant_b.clone(),
            disk.set_visibility(&path, openrust::storage::Visibility::Private),
        )
        .await??;
        assert_eq!(
            TenantContext::scope(tenant_b.clone(), disk.visibility(&path)).await??,
            openrust::storage::Visibility::Private
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let tenants = fixture.directory.path().join("tenants");
            let private_parents = std::fs::read_dir(&tenants)?
                .map(|entry| {
                    let entry = entry?;
                    let mode = entry.metadata()?.permissions().mode() & 0o777;
                    Ok::<_, std::io::Error>((entry.path(), mode))
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            assert_eq!(private_parents.len(), 2);
            assert!(private_parents.iter().all(|(_, mode)| *mode == 0o700));
            let public_tenant = TenantId::new("attachment-public-tenant")?;
            TenantContext::scope(public_tenant.clone(), async {
                disk.put_body(
                    path.clone(),
                    Some("text/plain".into()),
                    Body::from("published tenant object"),
                    UploadOptions {
                        visibility: Some(openrust::storage::Visibility::Public),
                        ..Default::default()
                    },
                    None,
                )
                .await
            })
            .await??;
            assert_eq!(
                TenantContext::scope(public_tenant.clone(), disk.visibility(&path)).await??,
                openrust::storage::Visibility::Public
            );
            for (parent, mode) in private_parents {
                assert_eq!(
                    std::fs::metadata(parent)?.permissions().mode() & 0o777,
                    mode
                );
            }
            let visibility_route = format!("{}/visibility", fixture.path());
            let foreign_owner_response = TenantContext::scope(
                public_tenant.clone(),
                fixture.app.request(request(
                    "PUT",
                    &visibility_route,
                    Some(&fixture.other_token),
                    "application/json",
                    Body::from(r#"{"visibility":"private"}"#),
                )?),
            )
            .await??;
            eprintln!(
                "STV_VISIBILITY phase=foreign_owner status={}",
                foreign_owner_response.status().as_u16()
            );
            foreign_owner_response.assert_status(StatusCode::NOT_FOUND)?;
            assert_eq!(
                TenantContext::scope(public_tenant.clone(), disk.visibility(&path)).await??,
                openrust::storage::Visibility::Public
            );
            let owner_response = TenantContext::scope(
                public_tenant.clone(),
                fixture.app.request(request(
                    "PUT",
                    &visibility_route,
                    Some(&fixture.owner_token),
                    "application/json",
                    Body::from(r#"{"visibility":"private"}"#),
                )?),
            )
            .await??;
            eprintln!(
                "STV_VISIBILITY phase=owner status={}",
                owner_response.status().as_u16()
            );
            owner_response.assert_status(StatusCode::NO_CONTENT)?;
            assert_eq!(
                TenantContext::scope(public_tenant, disk.visibility(&path)).await??,
                openrust::storage::Visibility::Private
            );
            assert_eq!(
                TenantContext::scope(tenant_b.clone(), disk.visibility(&path)).await??,
                openrust::storage::Visibility::Private
            );
            let mut other_reader =
                TenantContext::scope(tenant_b.clone(), disk.open(&path, None)).await??;
            let mut other_content = Vec::new();
            while let Some(chunk) = other_reader.body.next().await {
                other_content.extend_from_slice(&chunk?);
            }
            assert_eq!(other_content, b"tenant B object remains");
        }
        let transaction = connection.begin().await?;
        article::Entity::delete_by_id(id).exec(&transaction).await?;
        TenantContext::scope(tenant_a.clone(), async {
            attachments.record_deleted(&transaction, 1, id).await
        })
        .await??;
        transaction.commit().await?;
        let pending = attachment_cleanup::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .ok_or("missing tenant cleanup intent")?;
        assert_eq!(pending.tenant_id.as_deref(), Some(tenant_a.as_str()));
        let denied = TenantContext::scope(tenant_b.clone(), async {
            attachments.cleanup_deleted(1, id).await
        })
        .await?
        .err()
        .ok_or("foreign tenant cleanup should fail")?;
        assert_eq!(denied.code(), "example_attachment_cleanup_tenant_mismatch");
        assert!(
            attachment_cleanup::Entity::find_by_id(id)
                .one(&connection)
                .await?
                .is_some()
        );
        assert_eq!(
            TenantContext::scope(tenant_a.clone(), disk.stat(&path))
                .await??
                .size,
            15
        );
        assert_eq!(
            TenantContext::scope(tenant_b.clone(), disk.stat(&path))
                .await??
                .size,
            23
        );
        assert_eq!(
            TenantContext::scope(tenant_b.clone(), attachments.reconcile_pending())
                .await?
                .err()
                .ok_or("tenant maintenance should fail")?
                .code(),
            "example_attachment_cleanup_tenant_mismatch"
        );
        assert_eq!(attachments.reconcile_pending().await?, 1);
        assert!(
            attachment_cleanup::Entity::find_by_id(id)
                .one(&connection)
                .await?
                .is_none()
        );
        assert_eq!(
            TenantContext::scope(tenant_a, disk.stat(&path))
                .await?
                .err()
                .ok_or("tenant A object should be removed")?
                .code(),
            "storage_object_not_found"
        );
        assert_eq!(
            TenantContext::scope(tenant_b, disk.stat(&path))
                .await??
                .size,
            23
        );
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .catch_unwind()
    .await;
    let closed = std::panic::AssertUnwindSafe(fixture.close())
        .catch_unwind()
        .await;
    match result {
        Ok(Ok(())) => match closed {
            Ok(cleanup) => cleanup,
            Err(payload) => std::panic::resume_unwind(payload),
        },
        Ok(Err(error)) => {
            if !matches!(closed, Ok(Ok(()))) {
                eprintln!("STV_FIXTURE_CLEANUP secondary_failure");
            }
            Err(error)
        }
        Err(payload) => {
            if !matches!(closed, Ok(Ok(()))) {
                eprintln!("STV_FIXTURE_CLEANUP secondary_failure");
            }
            std::panic::resume_unwind(payload)
        }
    }
}

#[tokio::test]
async fn unscoped_cleanup_intent_cannot_inherit_maintenance_tenant() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let connection = db.primary_connection()?;
    fixture
        .app
        .request(request(
            "PUT",
            &fixture.path(),
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("unscoped object"),
        )?)
        .await?
        .assert_status(StatusCode::CREATED)?;
    let transaction = connection.begin().await?;
    article::Entity::delete_by_id(fixture.article_id)
        .exec(&transaction)
        .await?;
    attachments
        .record_deleted(&transaction, 1, fixture.article_id)
        .await?;
    transaction.commit().await?;
    let pending = attachment_cleanup::Entity::find_by_id(fixture.article_id)
        .one(&connection)
        .await?
        .ok_or("missing unscoped cleanup intent")?;
    assert_eq!(pending.tenant_id, None);
    let denied = TenantContext::scope(TenantId::new("attachment-tenant-b")?, async {
        attachments.cleanup_deleted(1, fixture.article_id).await
    })
    .await?
    .err()
    .ok_or("ambient tenant must not be inherited")?;
    assert_eq!(denied.code(), "example_attachment_cleanup_tenant_mismatch");
    assert!(fixture.object().exists());
    assert!(
        attachment_cleanup::Entity::find_by_id(fixture.article_id)
            .one(&connection)
            .await?
            .is_some()
    );
    assert_eq!(attachments.reconcile_pending().await?, 1);
    assert!(!fixture.object().exists());
    fixture.close().await
}

#[tokio::test]
async fn article_delete_waits_for_inflight_upload_then_cleans_object() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = Arc::clone(&started);
    let (release, continue_body) = tokio::sync::oneshot::channel::<()>();
    let body = Body::from_stream(
        futures_util::stream::once(async move {
            signal.notify_one();
            Ok::<_, std::io::Error>(Bytes::from_static(b"partial text"))
        })
        .chain(futures_util::stream::once(async move {
            continue_body.await.map_err(std::io::Error::other)?;
            Ok::<_, std::io::Error>(Bytes::from_static(b" completed"))
        })),
    );
    let id = fixture.article_id;
    let upload = tokio::spawn(async move {
        attachments
            .upload(
                1,
                id,
                AttachmentUpload::from_content_type("text/plain")?,
                body,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), started.notified()).await?;
    let article_path = format!("/api/example/articles/{id}");
    let deleted = {
        let deletion = fixture.app.request(request(
            "DELETE",
            &article_path,
            Some(&fixture.owner_token),
            "application/json",
            Body::empty(),
        )?);
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut deletion)
                .await
                .is_err()
        );
        release
            .send(())
            .map_err(|()| "upload body already closed")?;
        upload.await??;
        deletion.await?
    };
    deleted.assert_status(StatusCode::OK)?;
    assert!(!fixture.object().exists());
    fixture.close().await
}

#[tokio::test]
async fn cancelled_after_publication_cannot_orphan_object_or_delete_new_upload() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    attachments.pause_after_publish(Arc::clone(&reached), Arc::clone(&release))?;
    let id = fixture.article_id;
    let upload = tokio::spawn(async move {
        attachments
            .upload(
                1,
                id,
                AttachmentUpload::from_content_type("text/plain")?,
                Body::from("first complete object"),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), reached.notified()).await?;
    assert!(fixture.object().exists());
    upload.abort();
    assert!(upload.await.is_err());
    let article_path = format!("/api/example/articles/{id}");
    {
        let deletion = fixture.app.request(request(
            "DELETE",
            &article_path,
            Some(&fixture.owner_token),
            "application/json",
            Body::empty(),
        )?);
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut deletion)
                .await
                .is_err()
        );
        release.add_permits(1);
        deletion.as_mut().await?.assert_status(StatusCode::OK)?;
    }
    assert!(!fixture.object().exists());
    fixture.close().await
}

#[tokio::test]
async fn cancelled_attachment_upload_does_not_publish_partial_object() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = Arc::clone(&started);
    let body = Body::from_stream(
        futures_util::stream::once(async move {
            signal.notify_one();
            Ok::<_, std::io::Error>(Bytes::from_static(b"partial text"))
        })
        .chain(futures_util::stream::pending()),
    );
    let id = fixture.article_id;
    let upload = tokio::spawn(async move {
        attachments
            .upload(
                1,
                id,
                AttachmentUpload::from_content_type("text/plain")?,
                body,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), started.notified()).await?;
    upload.abort();
    assert!(upload.await.is_err());
    assert!(!fixture.object().exists());
    let deleted = fixture
        .app
        .request(request(
            "DELETE",
            &format!("/api/example/articles/{}", fixture.article_id),
            Some(&fixture.owner_token),
            "application/json",
            Body::empty(),
        )?)
        .await?;
    deleted.assert_status(StatusCode::OK)?;
    // 父应用关闭等待 framework 管理的资源，不把 staging Drop 说成远程协议已正常结束。
    fixture.close().await
}

#[tokio::test]
async fn upload_operation_deadline_releases_article_lock() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let attachments = fixture
        .app
        .app()?
        .state()
        .resolve::<ArticleAttachments>()
        .await?;
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = Arc::clone(&started);
    let body = Body::from_stream(
        futures_util::stream::once(async move {
            signal.notify_one();
            Ok::<_, std::io::Error>(Bytes::from_static(b"partial text"))
        })
        .chain(futures_util::stream::pending()),
    );
    let id = fixture.article_id;
    let upload = tokio::spawn(async move {
        attachments
            .upload_with_test_deadline(
                1,
                id,
                AttachmentUpload::from_content_type("text/plain")?,
                body,
                Duration::from_millis(50),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), started.notified()).await?;
    let error = upload
        .await?
        .err()
        .ok_or("pending upload should reach its operation deadline")?;
    assert_eq!(error.code(), "example_attachment_upload_deadline");
    assert!(!fixture.object().exists());

    let deleted = fixture
        .app
        .request(request(
            "DELETE",
            &format!("/api/example/articles/{id}"),
            Some(&fixture.owner_token),
            "application/json",
            Body::empty(),
        )?)
        .await?;
    deleted.assert_status(StatusCode::OK)?;
    fixture.close().await
}

#[tokio::test]
async fn text_inspector_checks_utf8_across_chunks_and_rejects_truncated_tail() -> Result<()> {
    let context = UploadContext {
        path: StoragePath::new("notes/example.txt")?,
        content_type: Some("text/plain".into()),
    };
    let inspector = TextAttachmentInspector::default();
    inspector.inspect_chunk(&context, &[0xe4]).await?;
    inspector
        .inspect_chunk(&context, &[0xb8, 0xad, b'\n'])
        .await?;
    inspector.finish(&context, 4).await?;
    let incomplete = TextAttachmentInspector::default();
    incomplete.inspect_chunk(&context, &[0xf0, 0x9f]).await?;
    assert!(incomplete.finish(&context, 2).await.is_err());
    let binary = TextAttachmentInspector::default();
    assert!(
        binary
            .inspect_chunk(&context, b"text\0hidden")
            .await
            .is_err()
    );
    Ok(())
}

async fn assert_access_rejections(fixture: &Fixture, path: &str) -> TestResult {
    fixture
        .app
        .request(request("GET", path, None, "text/plain", Body::empty())?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    for method in ["GET", "PUT", "DELETE"] {
        fixture
            .app
            .request(request(
                method,
                path,
                Some(&fixture.other_token),
                "text/plain",
                Body::from("not mine"),
            )?)
            .await?
            .assert_status(StatusCode::NOT_FOUND)?;
    }
    fixture
        .app
        .request(request(
            "PUT",
            "/api/articles/9223372036854775807/attachment",
            Some(&fixture.owner_token),
            "text/plain",
            Body::from("missing"),
        )?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    fixture
        .app
        .request(request(
            "PUT",
            path,
            Some(&fixture.owner_token),
            "image/png",
            Body::from("text"),
        )?)
        .await?
        .assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE)?;
    assert!(!fixture.object().exists());
    Ok(())
}

#[test]
fn upload_declaration_rejects_ambiguous_types_and_untrusted_lengths() -> Result<()> {
    use openrust::http::axum::http::{HeaderMap, HeaderValue, header};
    for mime in [
        "image/png",
        "text/html",
        "text/plain; charset=gbk",
        "text/plain; boundary=secret-sentinel",
    ] {
        let error = AttachmentUpload::from_content_type(mime)
            .err()
            .ok_or_else(|| Error::custom("test_expected_error", "无效 MIME 未被拒绝"))?;
        assert_eq!(error.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
    }
    let mut headers = HeaderMap::new();
    assert!(AttachmentUpload::from_headers(&headers).is_err());
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("1048577"));
    let error = AttachmentUpload::from_headers(&headers)
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "超限声明未被拒绝"))?;
    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
    headers.remove(header::CONTENT_LENGTH);
    headers.append(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    assert!(AttachmentUpload::from_headers(&headers).is_err());
    Ok(())
}
