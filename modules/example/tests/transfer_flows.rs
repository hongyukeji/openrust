//! 生产模块文件 API 的进程内 HTTP 回归；数据库由测试服务显式提供。
#![cfg(all(
    feature = "auth",
    feature = "data-exchange",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::http::requests::article_transfer::{ArticleFormat, MAX_TRANSFER_BYTES};
use super::models::article;
use super::services::article_transfer::encode_export;
use figment::{Figment, providers::Serialized};
use openrust::database::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, Statement,
};
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Db, Result};
use std::time::Duration;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
const IMPORT: &str = "/api/example/articles/import";
const EXPORT: &str = "/api/example/articles/export";
#[cfg(feature = "data-exchange-tasks")]
const EXPORT_JOBS: &str = "/api/example/articles/export-jobs";

#[test]
fn format_contract_rejects_ambiguous_mime_and_declared_oversize() -> Result<()> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    ArticleFormat::Csv.validate_headers(&headers)?;
    assert!(ArticleFormat::Xlsx.validate_headers(&headers).is_err());
    headers.append("content-type", HeaderValue::from_static("text/csv"));
    assert!(ArticleFormat::Csv.validate_headers(&headers).is_err());
    headers.remove("content-type");
    headers.insert("content-type", HeaderValue::from_static("text/csv"));
    headers.insert("content-length", HeaderValue::from_static("1048577"));
    assert_eq!(
        ArticleFormat::Csv
            .validate_headers(&headers)
            .err()
            .map(|e| e.code().to_owned()),
        Some("example_transfer_too_large".into())
    );
    Ok(())
}

struct TransferMigrator;
#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for TransferMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    directory: tempfile::TempDir,
    token: String,
    other: String,
}
impl Fixture {
    async fn create() -> std::result::Result<Option<Self>, Box<dyn std::error::Error>> {
        Self::create_with_default_disk("local").await
    }

    async fn create_with_default_disk(
        default_disk: &str,
    ) -> std::result::Result<Option<Self>, Box<dyn std::error::Error>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping article transfer DB test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<TransferMigrator>(&admin).await?;
        let directory = tempfile::tempdir()?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source = Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"}, "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"transfer-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "storage":{"default_disk":default_disk,"disks":{
                "local":{"driver":"fs","root":directory.path()},
                "alternate":{"driver":"fs","root":directory.path().join("alternate")}
            }}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let table = db.quoted_table_name("users")?;
        // Bearer 认证不依赖密码；已验证邮箱用于真实 ArticleWriter 的事务通知边界。
        for id in [1_i64, 2] {
            let sql = format!(
                "INSERT INTO {table} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'transfer-{id}@example.test','not-used',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"
            );
            connection
                .execute_raw(Statement::from_string(
                    connection.get_database_backend(),
                    sql,
                ))
                .await?;
        }
        for (owner, title) in [(1, "=SUM(1)"), (2, "OTHER_OWNER_SENTINEL")] {
            article::ActiveModel {
                owner_id: Set(owner),
                title: Set(title.into()),
                body: Set("Public body".into()),
                private_note_ciphertext: Set(Some("CIPHERTEXT_SENTINEL".into())),
                ..Default::default()
            }
            .insert(&connection)
            .await?;
        }
        Ok(Some(Self {
            token: super::selected_test_auth::seeded_password_access(app.app()?, 1).await?,
            other: super::selected_test_auth::seeded_password_access(app.app()?, 2).await?,
            app,
            database,
            directory,
        }))
    }
    async fn close(mut self) -> TestResult {
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.database.close().await?;
        Ok(())
    }
}
fn request(
    method: &str,
    path: &str,
    token: Option<&str>,
    format: ArticleFormat,
    bytes: Vec<u8>,
) -> Result<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", format.mime());
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder
        .body(Body::from(bytes))
        .map_err(openrust::Error::internal)
}
fn input_file(format: ArticleFormat) -> Result<Vec<u8>> {
    // 同一个 typed 映射分别经过 CSV 与 XLSX；无效短标题行必须留在安全报告中。
    let rows = [
        ("Valid title", "First body"),
        ("x", "Bad title"),
        ("Another title", "Second body"),
    ]
    .into_iter()
    .map(|(title, body)| {
        serde_json::Map::from_iter([
            ("title".to_owned(), serde_json::Value::from(title)),
            ("body".to_owned(), serde_json::Value::from(body)),
        ])
    })
    .collect();
    encode_export(format, rows)
}

#[tokio::test]
async fn csv_xlsx_import_share_writer_and_exports_are_owner_scoped() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    for (format, name) in [(ArticleFormat::Csv, "csv"), (ArticleFormat::Xlsx, "xlsx")] {
        let response = fixture
            .app
            .request(request(
                "POST",
                &format!("{IMPORT}?format={name}"),
                Some(&fixture.token),
                format,
                input_file(format)?,
            )?)
            .await?;
        response.assert_status(StatusCode::OK)?;
        let report: serde_json::Value = response.json()?;
        assert_eq!(report["data"]["rows_read"], 3);
        assert_eq!(report["data"]["rows_created"], 2);
        assert_eq!(report["data"]["rows_rejected"], 1);
        assert_eq!(report["data"]["issues"][0]["row"], 3);
        assert!(!String::from_utf8_lossy(response.body()).contains("Bad title"));
        let export = fixture
            .app
            .request(request(
                "GET",
                &format!("{EXPORT}?format={name}"),
                Some(&fixture.token),
                format,
                vec![],
            )?)
            .await?;
        export
            .assert_status(StatusCode::OK)?
            .assert_header("x-content-type-options", "nosniff")?;
        let imported_by_other = fixture
            .app
            .request(request(
                "POST",
                &format!("{IMPORT}?format={name}"),
                Some(&fixture.other),
                format,
                export.body().to_vec(),
            )?)
            .await?;
        imported_by_other.assert_status(StatusCode::OK)?;
        if matches!(format, ArticleFormat::Csv) {
            let text = std::str::from_utf8(export.body())?;
            assert!(text.contains("'=SUM(1)"));
            for forbidden in [
                "owner_id",
                "private_note",
                "CIPHERTEXT_SENTINEL",
                "OTHER_OWNER_SENTINEL",
            ] {
                assert!(!text.contains(forbidden));
            }
        }
    }
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let event_count = super::models::article_event::Entity::find()
        .filter(super::models::article_event::Column::OwnerId.eq(1))
        .count(&db.primary_connection()?)
        .await?;
    assert_eq!(event_count, 4);
    fixture.close().await
}

#[tokio::test]
async fn malformed_oversize_and_anonymous_imports_do_not_write() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let anonymous = fixture
        .app
        .request(request(
            "POST",
            IMPORT,
            None,
            ArticleFormat::Csv,
            b"title,body\nGood,body".to_vec(),
        )?)
        .await?;
    anonymous.assert_status(StatusCode::UNAUTHORIZED)?;
    for (path, format, bytes, expected) in [
        (
            IMPORT,
            ArticleFormat::Csv,
            vec![b'x'; MAX_TRANSFER_BYTES + 1],
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            "/api/example/articles/import?format=xlsx",
            ArticleFormat::Xlsx,
            b"not a zip".to_vec(),
            StatusCode::BAD_REQUEST,
        ),
        (
            IMPORT,
            ArticleFormat::Csv,
            format!("title,body\n{}", "Valid title,Body\n".repeat(101)).into_bytes(),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        fixture
            .app
            .request(request("POST", path, Some(&fixture.token), format, bytes)?)
            .await?
            .assert_status(expected)?;
    }
    let export = fixture
        .app
        .request(request(
            "GET",
            EXPORT,
            Some(&fixture.token),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    let text = std::str::from_utf8(export.body())?;
    assert_eq!(text.lines().count(), 2);
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
async fn queued_export_uses_production_worker_and_private_storage_download() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let queued = fixture
        .app
        .request(request(
            "POST",
            EXPORT_JOBS,
            Some(&fixture.token),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    queued.assert_status(StatusCode::CREATED)?;
    let queued: serde_json::Value = queued.json()?;
    let id = queued["data"]["job_id"]
        .as_str()
        .ok_or("missing export job id")?;
    let queue = fixture
        .app
        .app()?
        .state()
        .resolve::<openrust::queue::Queue>()
        .await?;
    assert!(
        queue
            .run_next_with_state("example", fixture.app.app()?.state())
            .await?
    );
    let path = format!("{EXPORT_JOBS}/{id}/1");
    let exported = fixture
        .app
        .request(request(
            "GET",
            &path,
            Some(&fixture.token),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    exported.assert_status(StatusCode::OK)?;
    let text = std::str::from_utf8(exported.body())?;
    assert!(text.contains("'=SUM(1)"));
    assert!(!text.contains("OTHER_OWNER_SENTINEL"));
    let denied = fixture
        .app
        .request(request(
            "GET",
            &path,
            Some(&fixture.other),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    assert!(!denied.status().is_success());
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
const IMPORT_JOBS: &str = "/api/example/articles/import-jobs";

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
async fn queue_import(
    fixture: &Fixture,
    bytes: Vec<u8>,
    format: ArticleFormat,
) -> std::result::Result<(String, String), Box<dyn std::error::Error>> {
    let suffix = match format {
        ArticleFormat::Csv => "csv",
        ArticleFormat::Xlsx => "xlsx",
    };
    let response = fixture
        .app
        .request(request(
            "POST",
            &format!("{IMPORT_JOBS}?format={suffix}"),
            Some(&fixture.token),
            format,
            bytes,
        )?)
        .await?;
    response.assert_status(StatusCode::CREATED)?;
    let response: serde_json::Value = response.json()?;
    Ok((
        response["data"]["import_id"]
            .as_str()
            .ok_or("missing import id")?
            .into(),
        response["data"]["job_id"]
            .as_str()
            .ok_or("missing import job id")?
            .into(),
    ))
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
async fn queued_csv_xlsx_import_reports_are_private_and_completed_retries_do_not_duplicate()
-> TestResult {
    use super::services::article_transfer::ArticleImportJob;
    use openrust::core::Cancellation;
    use openrust::queue::{Job, JobContext, JobRegistry, Queue};
    let Some(fixture) = Fixture::create_with_default_disk("alternate").await? else {
        return Ok(());
    };
    let state = fixture.app.app()?.state();
    let storage = state
        .resolve::<openrust::filesystem::FilesystemManager>()
        .await?;
    assert_eq!(storage.default_disk().await?.name(), "alternate");
    let queue = state.resolve::<Queue>().await?;
    let mut jobs = JobRegistry::default();
    jobs.register_job::<ArticleImportJob>()?;
    for format in [ArticleFormat::Csv, ArticleFormat::Xlsx] {
        let bytes = input_file(format)?;
        let (import_id, job_id) = queue_import(&fixture, bytes.clone(), format).await?;
        let source_key = format!("example/articles/1/imports/{import_id}/source");
        assert_eq!(
            tokio::fs::read(fixture.directory.path().join(&source_key)).await?,
            bytes
        );
        assert!(
            !tokio::fs::try_exists(fixture.directory.path().join("alternate").join(&source_key))
                .await?
        );
        assert!(
            queue
                .run_next_with_state("example-import", fixture.app.app()?.state())
                .await?
        );
        let path = format!("{IMPORT_JOBS}/{import_id}/{job_id}/1");
        let response = fixture
            .app
            .request(request(
                "GET",
                &path,
                Some(&fixture.token),
                ArticleFormat::Csv,
                vec![],
            )?)
            .await?;
        response
            .assert_status(StatusCode::OK)?
            .assert_header("cache-control", "private, no-store")?;
        let report: serde_json::Value = response.json()?;
        assert_eq!(report["rows_committed"], 2);
        assert_eq!(report["rows_invalid"], 1);
        assert!(report["failure_code"].is_null());
        assert!(!String::from_utf8_lossy(response.body()).contains("Bad title"));
        let denied = fixture
            .app
            .request(request(
                "GET",
                &path,
                Some(&fixture.other),
                ArticleFormat::Csv,
                vec![],
            )?)
            .await?;
        assert!(!denied.status().is_success());
        // 完成后的同一导入 ID 在24小时幂等窗口内重放原结果，不再次发事件或通知。
        let context = JobContext::new(&job_id, 2, 3, Cancellation::new())?
            .with_app_state(fixture.app.app()?.state().clone())
            .with_progress_reporter(|_| Box::pin(async { Ok(()) }));
        jobs.execute(
            ArticleImportJob::NAME,
            serde_json::to_value(ArticleImportJob {
                owner_id: 1,
                import_id,
                format,
            })?,
            context,
        )
        .await?;
    }
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    assert_eq!(
        super::models::article_event::Entity::find()
            .filter(super::models::article_event::Column::OwnerId.eq(1))
            .count(&db.primary_connection()?)
            .await?,
        4
    );
    assert_eq!(
        article::Entity::find()
            .filter(article::Column::OwnerId.eq(1))
            .count(&db.primary_connection()?)
            .await?,
        5
    );
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn queued_import_failed_batch_rolls_back_and_retry_keeps_committed_batches() -> TestResult {
    use super::services::article_transfer::ArticleImportJob;
    use openrust::core::Cancellation;
    use openrust::queue::{Job, JobContext, JobRegistry, Queue};
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let connection = db.primary_connection()?;
    connection.execute_raw(Statement::from_string(connection.get_database_backend(),
        "ALTER TABLE example_articles ADD CONSTRAINT chk_transfer_title CHECK (title <> 'Fail write')".to_owned())).await?;
    let (import_id, job_id) = queue_import(
        &fixture,
        b"title,body\nFirst valid,body\nSecond valid,body\nFail write,body\nFourth valid,body\n"
            .to_vec(),
        ArticleFormat::Csv,
    )
    .await?;
    let queue = fixture.app.app()?.state().resolve::<Queue>().await?;
    assert!(
        queue
            .run_next_with_state("example-import", fixture.app.app()?.state())
            .await?
    );
    let report_path = format!("{IMPORT_JOBS}/{import_id}/{job_id}/1");
    let report = fixture
        .app
        .request(request(
            "GET",
            &report_path,
            Some(&fixture.token),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    report.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = report.json()?;
    assert_eq!(body["rows_committed"], 2);
    assert!(body["failure_code"].is_string());
    assert!(!String::from_utf8_lossy(report.body()).contains("Fail write"));
    assert_eq!(
        article::Entity::find()
            .filter(article::Column::OwnerId.eq(1))
            .count(&connection)
            .await?,
        3
    );
    // 可重试失败仍是 open；清理不得删除下一次 attempt 所需的来源。
    let receipt = super::models::article_import::Entity::find_by_id(&import_id)
        .one(&connection)
        .await?
        .ok_or("missing retry receipt")?;
    assert_eq!(receipt.state, "open");
    run_cleanup(&fixture, 1, &import_id, 0, receipt.created_at_ms + 1).await?;
    assert!(fixture.directory.path().join(&receipt.source_key).exists());
    let sql = match connection.get_database_backend() {
        openrust::database::sea_orm::DbBackend::MySql => {
            "ALTER TABLE example_articles DROP CHECK chk_transfer_title"
        }
        _ => "ALTER TABLE example_articles DROP CONSTRAINT chk_transfer_title",
    };
    connection
        .execute_raw(Statement::from_string(
            connection.get_database_backend(),
            sql.to_owned(),
        ))
        .await?;
    let mut jobs = JobRegistry::default();
    jobs.register_job::<ArticleImportJob>()?;
    let context = JobContext::new(&job_id, 2, 3, Cancellation::new())?
        .with_app_state(fixture.app.app()?.state().clone())
        .with_progress_reporter(|_| Box::pin(async { Ok(()) }));
    jobs.execute(
        ArticleImportJob::NAME,
        serde_json::to_value(ArticleImportJob {
            owner_id: 1,
            import_id: import_id.clone(),
            format: ArticleFormat::Csv,
        })?,
        context,
    )
    .await?;
    let report_path = format!("{IMPORT_JOBS}/{import_id}/{job_id}/2");
    let report = fixture
        .app
        .request(request(
            "GET",
            &report_path,
            Some(&fixture.token),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    report.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = report.json()?;
    assert_eq!(body["rows_committed"], 4);
    assert!(body["failure_code"].is_null());
    assert_eq!(
        article::Entity::find()
            .filter(article::Column::OwnerId.eq(1))
            .count(&connection)
            .await?,
        5
    );
    assert_eq!(
        super::models::article_event::Entity::find()
            .filter(super::models::article_event::Column::OwnerId.eq(1))
            .count(&connection)
            .await?,
        4
    );
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
fn cleanup_context(fixture: &Fixture) -> Result<openrust::queue::JobContext> {
    Ok(openrust::queue::JobContext::new(
        "source-cleanup-test",
        1,
        3,
        openrust::core::Cancellation::new(),
    )?
    .with_app_state(fixture.app.app()?.state().clone()))
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
async fn run_cleanup(
    fixture: &Fixture,
    owner: i64,
    id: &str,
    deferrals: u8,
    now: i64,
) -> Result<()> {
    let attempt = cleanup_context(fixture)?.begin_attempt()?;
    let result = super::services::article_transfer::cleanup_import_at(
        attempt.context().clone(),
        owner,
        id,
        deferrals,
        now,
    )
    .await;
    let cleanup = attempt
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    result?;
    cleanup
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
async fn source_cleanup_retains_pending_and_running_import_then_removes_only_terminal_source()
-> TestResult {
    use super::models::article_import;
    use super::services::article_transfer::ArticleImportJob;
    use openrust::database::{LockAcquire, SharedLocks};
    use openrust::queue::{Job, JobContext, JobRegistry, Queue};
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let (id, job_id) = queue_import(
        &fixture,
        input_file(ArticleFormat::Csv)?,
        ArticleFormat::Csv,
    )
    .await?;
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let row = article_import::Entity::find_by_id(&id)
        .one(&db.primary_connection()?)
        .await?
        .ok_or("missing receipt")?;
    let source = fixture.directory.path().join(&row.source_key);
    assert!(source.exists());
    run_cleanup(&fixture, 1, &id, 0, row.created_at_ms + 1).await?;
    assert!(source.exists());
    // 实际持有同一个分布式租约模拟运行中任务；即使清理时钟跨24小时也不得删输入。
    let locks = SharedLocks::new((*db).clone());
    let lease = match locks
        .acquire(
            &format!("example:article-import:1:{id}"),
            Duration::from_secs(300),
        )
        .await?
    {
        LockAcquire::Acquired(lease) => lease,
        LockAcquire::Busy { .. } => return Err("test import lock unexpectedly busy".into()),
    };
    run_cleanup(&fixture, 1, &id, 0, row.created_at_ms + 24 * 3600 * 1000).await?;
    assert!(source.exists());
    assert_eq!(
        article_import::Entity::find_by_id(&id)
            .one(&db.primary_connection()?)
            .await?
            .ok_or("missing receipt")?
            .state,
        "open"
    );
    assert!(locks.release(&lease).await?);
    let queue = fixture.app.app()?.state().resolve::<Queue>().await?;
    assert!(
        queue
            .run_next_with_state("example-import", fixture.app.app()?.state())
            .await?
    );
    run_cleanup(&fixture, 1, &id, 0, row.created_at_ms + 2).await?;
    assert!(!source.exists());
    let terminal = article_import::Entity::find_by_id(&id)
        .one(&db.primary_connection()?)
        .await?
        .ok_or("missing terminal receipt")?;
    assert_eq!(terminal.state, "completed");
    assert!(terminal.source_removed);
    // 原 payload 在输入清理后直接走持久终态，不会读取已删除来源或再次写文章。
    let mut jobs = JobRegistry::default();
    jobs.register_job::<ArticleImportJob>()?;
    jobs.execute(
        ArticleImportJob::NAME,
        serde_json::to_value(ArticleImportJob {
            owner_id: 1,
            import_id: id,
            format: ArticleFormat::Csv,
        })?,
        JobContext::new(job_id, 2, 3, openrust::core::Cancellation::new())?
            .with_app_state(fixture.app.app()?.state().clone()),
    )
    .await?;
    assert_eq!(
        article::Entity::find()
            .filter(article::Column::OwnerId.eq(1))
            .count(&db.primary_connection()?)
            .await?,
        3
    );
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
async fn expired_pending_import_has_visible_terminal_state_and_never_reads_removed_source()
-> TestResult {
    use super::models::article_import;
    use super::services::article_transfer::ArticleImportJob;
    use openrust::queue::{Job, JobContext, JobRegistry};
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let (id, job_id) = queue_import(
        &fixture,
        input_file(ArticleFormat::Csv)?,
        ArticleFormat::Csv,
    )
    .await?;
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let row = article_import::Entity::find_by_id(&id)
        .one(&db.primary_connection()?)
        .await?
        .ok_or("missing receipt")?;
    run_cleanup(&fixture, 1, &id, 0, row.created_at_ms + 24 * 3600 * 1000).await?;
    let status_path = format!("{IMPORT_JOBS}/{id}/status");
    let status = fixture
        .app
        .request(request(
            "GET",
            &status_path,
            Some(&fixture.token),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?;
    status.assert_status(StatusCode::OK)?;
    let status: serde_json::Value = status.json()?;
    assert_eq!(status["data"]["state"], "failed");
    assert_eq!(status["data"]["error_code"], "example_import_expired");
    assert_eq!(status["data"]["source_removed"], true);
    fixture
        .app
        .request(request(
            "GET",
            &status_path,
            Some(&fixture.other),
            ArticleFormat::Csv,
            vec![],
        )?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    let mut jobs = JobRegistry::default();
    jobs.register_job::<ArticleImportJob>()?;
    let error = jobs
        .execute(
            ArticleImportJob::NAME,
            serde_json::to_value(ArticleImportJob {
                owner_id: 1,
                import_id: id,
                format: ArticleFormat::Csv,
            })?,
            JobContext::new(job_id, 1, 3, openrust::core::Cancellation::new())?
                .with_app_state(fixture.app.app()?.state().clone()),
        )
        .await
        .err()
        .ok_or("expired import unexpectedly ran")?;
    assert_eq!(error.code(), "example_import_closed");
    assert_eq!(
        article::Entity::find()
            .filter(article::Column::OwnerId.eq(1))
            .count(&db.primary_connection()?)
            .await?,
        1
    );
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
async fn import_reconcile_recovers_missing_cleanup_job_and_exhausted_deferrals() -> TestResult {
    use super::models::article_import;
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let import_id = uuid::Uuid::now_v7().to_string();
    let source_key = format!("example/articles/1/imports/{import_id}/source");
    let source = fixture.directory.path().join(&source_key);
    std::fs::create_dir_all(source.parent().ok_or("source has no parent")?)?;
    std::fs::write(&source, b"title,body\nValid title,Valid body\n")?;
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    let created = now - 24 * 3600 * 1000 - 1;
    article_import::ActiveModel {
        import_id: Set(import_id.clone()),
        owner_id: Set(1),
        source_key: Set(source_key),
        format: Set("csv".into()),
        state: Set("open".into()),
        error_code: Set(None),
        created_at_ms: Set(created),
        terminal_at_ms: Set(None),
        source_removed: Set(false),
    }
    .insert(&db.primary_connection()?)
    .await?;
    // 没有任何初始清理任务；八次延期后的任务也只返回诊断，不删除来源。
    let deferred = run_cleanup(&fixture, 1, &import_id, 8, created + 1)
        .await
        .err()
        .ok_or("expected deferred cleanup")?;
    assert_eq!(deferred.code(), "example_import_cleanup_deferred");
    assert!(source.exists());
    // 前一页 100 条未过期来源不得让后一页已过期来源永久饥饿。
    for index in 1..=100_u128 {
        let id = uuid::Uuid::from_u128(index).to_string();
        article_import::ActiveModel {
            import_id: Set(id.clone()),
            owner_id: Set(1),
            source_key: Set(format!("example/articles/1/imports/{id}/source")),
            format: Set("csv".into()),
            state: Set("open".into()),
            error_code: Set(None),
            created_at_ms: Set(now),
            terminal_at_ms: Set(None),
            source_removed: Set(false),
        }
        .insert(&db.primary_connection()?)
        .await?;
    }
    fixture
        .app
        .app()?
        .run_command("example", vec!["imports".into(), "reconcile".into()])
        .await?;
    assert!(source.exists());
    fixture
        .app
        .app()?
        .run_command(
            "example",
            vec![
                "imports".into(),
                "reconcile".into(),
                uuid::Uuid::from_u128(100).to_string(),
            ],
        )
        .await?;
    assert!(!source.exists());
    let receipt = article_import::Entity::find_by_id(&import_id)
        .one(&db.primary_connection()?)
        .await?
        .ok_or("missing import receipt")?;
    assert_eq!(receipt.state, "failed");
    assert_eq!(
        receipt.error_code.as_deref(),
        Some("example_import_expired")
    );
    assert!(receipt.source_removed);
    fixture
        .app
        .app()?
        .run_command(
            "example",
            vec![
                "imports".into(),
                "reconcile".into(),
                uuid::Uuid::from_u128(100).to_string(),
            ],
        )
        .await?;
    fixture.close().await
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn real_tcp_import_upload_worker_and_owner_report() -> TestResult {
    use openrust::queue::Queue;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;

    async fn send(
        address: std::net::SocketAddr,
        method: &str,
        path: &str,
        token: &str,
        content_type: &str,
        body: &[u8],
    ) -> std::result::Result<(StatusCode, serde_json::Value), Box<dyn std::error::Error>> {
        let mut stream = TcpStream::connect(address).await?;
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(request.as_bytes()).await?;
        stream.write_all(body).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        let response = String::from_utf8(response)?;
        let (head, data) = response.split_once("\r\n\r\n").ok_or("missing HTTP body")?;
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_ascii_whitespace().nth(1))
            .ok_or("missing HTTP status")?
            .parse::<u16>()?;
        let data = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            let mut remainder = data;
            let mut decoded = String::new();
            loop {
                let (size, rest) = remainder
                    .split_once("\r\n")
                    .ok_or("missing HTTP chunk size")?;
                let length = usize::from_str_radix(
                    size.split(';').next().ok_or("bad HTTP chunk size")?,
                    16,
                )?;
                if length == 0 {
                    break;
                }
                decoded.push_str(rest.get(..length).ok_or("short HTTP chunk")?);
                remainder = rest
                    .get(length + 2..)
                    .ok_or("missing HTTP chunk separator")?;
            }
            decoded
        } else {
            data.to_owned()
        };
        Ok((StatusCode::from_u16(status)?, serde_json::from_str(&data)?))
    }

    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = openrust::foundation::http::build_router(fixture.app.app()?)?;
    let (stop, stopped) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        openrust::http::axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
    });
    let input = input_file(ArticleFormat::Csv)?;
    let (status, queued) = send(
        address,
        "POST",
        &format!("{IMPORT_JOBS}?format=csv"),
        &fixture.token,
        "text/csv",
        &input,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let import_id = queued["data"]["import_id"]
        .as_str()
        .ok_or("missing import ID")?;
    let job_id = queued["data"]["job_id"].as_str().ok_or("missing job ID")?;
    let queue = fixture.app.app()?.state().resolve::<Queue>().await?;
    assert!(
        queue
            .run_next_with_state("example-import", fixture.app.app()?.state())
            .await?
    );
    let report = format!("{IMPORT_JOBS}/{import_id}/{job_id}/1");
    let (status, owner) = send(
        address,
        "GET",
        &report,
        &fixture.token,
        "application/json",
        &[],
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner["rows_committed"], 2);
    let (status, _) = send(
        address,
        "GET",
        &report,
        &fixture.other,
        "application/json",
        &[],
    )
    .await?;
    assert!(!status.is_success());
    let status_path = format!("{IMPORT_JOBS}/{import_id}/status");
    let (status, state) = send(
        address,
        "GET",
        &status_path,
        &fixture.token,
        "application/json",
        &[],
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["data"]["state"], "completed");
    let _ = stop.send(());
    server.await??;
    fixture.close().await
}
