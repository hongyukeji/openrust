//! 在真实 PostgreSQL/MySQL 隔离库中验证任务化数据交换。
#![cfg(all(
    feature = "data-exchange-tasks",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#![allow(clippy::expect_used, reason = "集成测试夹具错误时应立即终止")]

use std::{
    fmt::Write as _,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use figment::{
    Figment,
    providers::{Format, Toml},
};
use futures_util::{FutureExt, StreamExt, stream};
use openrust::Cancellation;
use openrust::core::Error;
use openrust::core::Result as FrameworkResult;
use openrust::data::exchange::CellType;
use openrust::data::exchange::ExportOptions;
use openrust::data::exchange::ImportColumn;
use openrust::data::exchange::ImportLimits;
use openrust::data::exchange::ImportSchema;
use openrust::data::exchange::tasks::ExchangeFormat;
use openrust::data::exchange::tasks::ExportTaskOptions;
use openrust::data::exchange::tasks::ImportReportCleanupJob;
use openrust::data::exchange::tasks::ImportTaskInput;
use openrust::data::exchange::tasks::ImportTaskOptions;
use openrust::data::exchange::tasks::ImportTaskReport;
use openrust::data::exchange::tasks::export_storage_task;
use openrust::data::exchange::tasks::import_storage_task;
use openrust::database::CoordinationMigration;
use openrust::database::DatabaseServiceProvider;
use openrust::database::DatabaseTransaction;
use openrust::database::Db;
use openrust::database::DbBackend;
use openrust::database::SharedLocks;
use openrust::database::sea_orm::ConnectionTrait;
use openrust::database::sea_orm::Statement;
use openrust::database::sea_orm_migration::MigrationTrait;
use openrust::database::sea_orm_migration::MigratorTrait;
use openrust::filesystem::FilesystemManager;
use openrust::filesystem::FilesystemServiceProvider;
use openrust::filesystem::StoragePath;
use openrust::filesystem::UploadOptions;
use openrust::foundation::Application;
use openrust::foundation::ProviderBuilder;
use openrust::queue::DispatchOptions;
use openrust::queue::Job;
use openrust::queue::JobContext;
use openrust::queue::Queue;
use openrust::queue::QueueMigration;
use openrust::queue::QueueServiceProvider;
use openrust::support::BoxFuture;
use openrust::testing::TestDatabase;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

struct ExchangeMigrator;
static FAIL_NEXT_BATCH: AtomicBool = AtomicBool::new(false);
static CANCEL_IMPORT: OnceLock<Cancellation> = OnceLock::new();

impl MigratorTrait for ExchangeMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration), Box::new(CoordinationMigration)]
    }
}

async fn build_app(
    admin_url: &str,
) -> FrameworkResult<(Application, TestDatabase, tempfile::TempDir)> {
    let database = TestDatabase::create::<ExchangeMigrator>(admin_url).await?;
    let storage_root = tempfile::tempdir().map_err(Error::internal)?;
    let figment = Figment::new().merge(Toml::string(&format!(
        "[database]\nurl = {:?}\n[storage]\ndefault_disk = 'local'\nmax_chunk_bytes = 4096\n[storage.disks.local]\ndriver = 'fs'\nroot = {:?}\n",
        database.url(),
        storage_root.path().to_string_lossy()
    )));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(QueueServiceProvider::new())
        .provider(FilesystemServiceProvider::new());
    let app = Application::from_provider_runtime(builder.start(figment).await?)?;
    let db = app.state().resolve::<Db>().await?;
    db.connection()?
        .execute_unprepared("CREATE TABLE exchange_task_rows (email VARCHAR(255) NOT NULL)")
        .await
        .map_err(Error::internal)?;
    Ok((app, database, storage_root))
}

fn context(app: &Application, id: &str) -> FrameworkResult<JobContext> {
    Ok(JobContext::new(id, 1, 3, Cancellation::new())?
        .with_app_state(app.state().clone())
        .with_progress_reporter(|_| Box::pin(async { Ok(()) })))
}

fn context_with_progress(
    app: &Application,
    id: &str,
    progress: std::sync::Arc<tokio::sync::Mutex<Vec<Value>>>,
) -> FrameworkResult<JobContext> {
    Ok(JobContext::new(id, 1, 3, Cancellation::new())?
        .with_app_state(app.state().clone())
        .with_progress_reporter(move |value| {
            let progress = std::sync::Arc::clone(&progress);
            Box::pin(async move {
                progress.lock().await.push(value);
                Ok(())
            })
        }))
}

fn finish_attempt<T>(
    result: FrameworkResult<T>,
    closed: FrameworkResult<()>,
) -> FrameworkResult<T> {
    match result {
        Ok(value) => {
            closed?;
            Ok(value)
        }
        Err(error) => {
            if closed.is_err() {
                eprintln!("STV_DATA_EXCHANGE_ATTEMPT secondary_cleanup_failure");
            }
            Err(error)
        }
    }
}

async fn import_in_attempt<F>(
    context: JobContext,
    input: ImportTaskInput,
    options: &ImportTaskOptions,
    persist_batch: F,
) -> FrameworkResult<ImportTaskReport>
where
    F: for<'a> FnMut(
            &'a DatabaseTransaction,
            Vec<Map<String, Value>>,
        ) -> BoxFuture<'a, FrameworkResult<()>>
        + Send
        + 'static,
{
    let attempt = context.begin_attempt()?;
    let result = import_storage_task(attempt.context(), input, options, persist_batch).await;
    let closed = attempt
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    finish_attempt(result, closed)
}

async fn import_error_in_attempt<F>(
    context: JobContext,
    input: ImportTaskInput,
    options: &ImportTaskOptions,
    persist_batch: F,
) -> FrameworkResult<Error>
where
    F: for<'a> FnMut(
            &'a DatabaseTransaction,
            Vec<Map<String, Value>>,
        ) -> BoxFuture<'a, FrameworkResult<()>>
        + Send
        + 'static,
{
    let attempt = context.begin_attempt()?;
    let result = import_storage_task(attempt.context(), input, options, persist_batch).await;
    let closed = attempt
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    closed?;
    result.err().ok_or_else(|| {
        Error::custom(
            "test_expected_import_failure",
            "import task unexpectedly succeeded",
        )
    })
}

async fn put_source(
    app: &Application,
    path: &StoragePath,
    source: &'static [u8],
) -> FrameworkResult<()> {
    app.state()
        .resolve::<FilesystemManager>()
        .await?
        .disk("local")
        .await?
        .put_stream(
            path.clone(),
            Some("text/csv".to_owned()),
            Box::pin(stream::iter([Ok(Bytes::from_static(source))])),
            UploadOptions {
                visibility: None,
                max_bytes: 1024 * 1024,
                max_chunk_bytes: 4096,
                allowed_content_types: vec!["text/csv".to_owned()],
            },
            None,
        )
        .await?;
    Ok(())
}

fn persist_rows(
    transaction: &DatabaseTransaction,
    rows: Vec<Map<String, Value>>,
) -> BoxFuture<'_, FrameworkResult<()>> {
    Box::pin(async move {
        let backend = transaction.get_database_backend();
        for row in rows {
            let email = row
                .get("email")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::custom("test_row_invalid", "email is missing"))?;
            let sql = match backend {
                DbBackend::Postgres => "INSERT INTO exchange_task_rows (email) VALUES ($1)",
                DbBackend::MySql => "INSERT INTO exchange_task_rows (email) VALUES (?)",
                _ => return Err(Error::custom("test_backend_invalid", "unexpected backend")),
            };
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    sql,
                    [email.to_owned().into()],
                ))
                .await
                .map_err(Error::internal)?;
        }
        Ok(())
    })
}

fn persist_rows_fail_once(
    transaction: &DatabaseTransaction,
    rows: Vec<Map<String, Value>>,
) -> BoxFuture<'_, FrameworkResult<()>> {
    Box::pin(async move {
        if rows
            .iter()
            .any(|row| row.get("email") == Some(&json!("b@example.test")))
            && FAIL_NEXT_BATCH.swap(false, Ordering::SeqCst)
        {
            return Err(Error::custom(
                "test_batch_failure",
                "injected batch failure",
            ));
        }
        persist_rows(transaction, rows).await
    })
}

fn persist_rows_then_cancel(
    transaction: &DatabaseTransaction,
    rows: Vec<Map<String, Value>>,
) -> BoxFuture<'_, FrameworkResult<()>> {
    Box::pin(async move {
        persist_rows(transaction, rows).await?;
        CANCEL_IMPORT
            .get()
            .ok_or_else(|| Error::custom("test_cancel_missing", "cancellation is not configured"))?
            .cancel();
        Ok(())
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "同一真实隔离库验证导入重试、租户隔离与导出生命周期"
)]
async fn run_data_exchange_flow(app: &Application) -> Result<(), Box<dyn std::error::Error>> {
    let storage = app.state().resolve::<FilesystemManager>().await?;
    let input_path = StoragePath::new("private/import/source.csv")?;
    put_source(app, &input_path, b"email\na@example.test\nb@example.test\n").await?;
    let schema = ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?;
    let progress = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let options = ImportTaskOptions {
        batch_size: 1,
        max_batch_bytes: 1024 * 1024,
        limits: ImportLimits::default(),
        report_disk: "local".to_owned(),
        report_path: StoragePath::new("private/reports/import.json")?,
        report_retention: Duration::from_secs(60),
    };
    let report = import_in_attempt(
        context_with_progress(app, "import-first", std::sync::Arc::clone(&progress))?,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: None,
            import_id: "customer-import-1".to_owned(),
            format: ExchangeFormat::Csv,
            schema: schema.clone(),
        },
        &options,
        persist_rows,
    )
    .await?;
    assert_eq!(report.rows_committed, 2);
    assert_eq!(report.batches_committed, 2);
    assert!(
        progress
            .lock()
            .await
            .iter()
            .any(|value| { value.get("batches_committed").and_then(Value::as_u64) == Some(2) })
    );
    let queue = app.state().resolve::<Queue>().await?;
    assert!(
        !queue
            .run_next_with_state("data-exchange-cleanup", app.state())
            .await?,
        "未到保留期限的清理任务不能提前执行"
    );

    let replay = import_in_attempt(
        context(app, "import-replay")?,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: None,
            import_id: "customer-import-1".to_owned(),
            format: ExchangeFormat::Csv,
            schema,
        },
        &options,
        persist_rows,
    )
    .await?;
    assert_eq!(replay, report);

    let alternate_path = StoragePath::new("private/import/alternate.csv")?;
    put_source(app, &alternate_path, b"email\nother@example.test\n").await?;
    let conflict = import_error_in_attempt(
        context(app, "import-source-conflict")?,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: alternate_path,
            version: None,
            import_id: "customer-import-1".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?,
        },
        &options,
        persist_rows,
    )
    .await?;
    assert_eq!(conflict.code(), "data_exchange_import_conflict");

    let _failure = import_error_in_attempt(
        context(app, "import-source-version")?,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: Some("version-that-does-not-exist".to_owned()),
            import_id: "customer-import-version".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?,
        },
        &options,
        persist_rows,
    )
    .await?;

    let failure_options = ImportTaskOptions {
        report_path: StoragePath::new("private/reports/partial.json")?,
        ..options.clone()
    };
    let partial_context = context(app, "import-partial")?;
    let partial_report_path = failure_options.report_path_for_attempt(
        partial_context.id(),
        partial_context.attempt(),
        "customer-import-partial",
    )?;
    FAIL_NEXT_BATCH.store(true, Ordering::SeqCst);
    let _failure = import_error_in_attempt(
        partial_context,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: None,
            import_id: "customer-import-partial".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?,
        },
        &failure_options,
        persist_rows_fail_once,
    )
    .await?;
    let mut partial_reader = storage
        .disk("local")
        .await?
        .open(&partial_report_path, None)
        .await?;
    let mut partial_bytes = Vec::new();
    while let Some(chunk) = partial_reader.body.next().await {
        partial_bytes.extend_from_slice(&chunk?);
    }
    let partial_report: ImportTaskReport = serde_json::from_slice(&partial_bytes)?;
    assert_eq!(partial_report.rows_committed, 1);
    assert!(partial_report.failure_code.is_some());
    let recover_context = context(app, "import-recover")?;
    let recovery_report_path = failure_options.report_path_for_attempt(
        recover_context.id(),
        recover_context.attempt(),
        "customer-import-partial",
    )?;
    assert_ne!(recovery_report_path, partial_report_path);
    let recovered = import_in_attempt(
        recover_context,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: None,
            import_id: "customer-import-partial".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?,
        },
        &failure_options,
        persist_rows,
    )
    .await?;
    assert_eq!(recovered.rows_committed, 2);
    assert_eq!(recovered.report_path, recovery_report_path.as_str());

    let cancellation = Cancellation::new();
    CANCEL_IMPORT
        .set(cancellation.clone())
        .map_err(|_| "test cancellation was already configured")?;
    let cancelled_options = ImportTaskOptions {
        report_path: StoragePath::new("private/reports/cancelled-import.json")?,
        ..options.clone()
    };
    let cancelled_context = JobContext::new("import-cancelled", 1, 3, cancellation)?
        .with_app_state(app.state().clone())
        .with_progress_reporter(|_| Box::pin(async { Ok(()) }));
    let cancelled_report_path = cancelled_options.report_path_for_attempt(
        cancelled_context.id(),
        cancelled_context.attempt(),
        "customer-import-cancelled",
    )?;
    let _failure = import_error_in_attempt(
        cancelled_context,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: None,
            import_id: "customer-import-cancelled".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?,
        },
        &cancelled_options,
        persist_rows_then_cancel,
    )
    .await?;
    let mut cancelled_reader = storage
        .disk("local")
        .await?
        .open(&cancelled_report_path, None)
        .await?;
    let mut cancelled_bytes = Vec::new();
    while let Some(chunk) = cancelled_reader.body.next().await {
        cancelled_bytes.extend_from_slice(&chunk?);
    }
    let cancelled_report: ImportTaskReport = serde_json::from_slice(&cancelled_bytes)?;
    assert!(cancelled_report.cancelled);
    assert_eq!(cancelled_report.rows_committed, 1);
    let resumed = import_in_attempt(
        context(app, "import-cancel-resume")?,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: input_path.clone(),
            version: None,
            import_id: "customer-import-cancelled".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![ImportColumn::new("email", "email", CellType::Text)])?,
        },
        &cancelled_options,
        persist_rows,
    )
    .await?;
    assert_eq!(resumed.rows_committed, 2);

    let invalid_path = StoragePath::new("private/import/invalid.csv")?;
    put_source(
        app,
        &invalid_path,
        b"email,age\nbad@example.test,nope\nvalid@example.test,42\n",
    )
    .await?;
    let invalid_options = ImportTaskOptions {
        report_path: StoragePath::new("private/reports/validation-errors.json")?,
        ..options.clone()
    };
    let invalid_report = import_in_attempt(
        context(app, "import-invalid-row")?,
        ImportTaskInput {
            disk: "local".to_owned(),
            path: invalid_path,
            version: None,
            import_id: "customer-import-validation".to_owned(),
            format: ExchangeFormat::Csv,
            schema: ImportSchema::new(vec![
                ImportColumn::new("email", "email", CellType::Text),
                ImportColumn::new("age", "age", CellType::Integer),
            ])?,
        },
        &invalid_options,
        persist_rows,
    )
    .await?;
    assert_eq!(invalid_report.rows_invalid, 1);
    assert_eq!(invalid_report.rows_committed, 1);
    assert_eq!(invalid_report.errors.len(), 1);
    assert!(!serde_json::to_string(&invalid_report)?.contains("nope"));

    for tenant in ["tenant-a", "tenant-b"] {
        let tenant_id = openrust::tenant::TenantId::new(tenant)?;
        let tenant_path = StoragePath::new(format!("private/import/{tenant}.csv"))?;
        openrust::tenant::TenantContext::scope(tenant_id.clone(), async {
            put_source(
                app,
                &tenant_path,
                b"email\na@example.test\nb@example.test\n",
            )
            .await
        })
        .await??;
        let tenant_options = ImportTaskOptions {
            report_path: StoragePath::new(format!("private/reports/{tenant}.json"))?,
            ..options.clone()
        };
        let tenant_report = openrust::tenant::TenantContext::scope(
            tenant_id,
            import_in_attempt(
                context(app, &format!("import-{tenant}"))?,
                ImportTaskInput {
                    disk: "local".to_owned(),
                    path: tenant_path,
                    version: None,
                    import_id: "shared-import-id".to_owned(),
                    format: ExchangeFormat::Csv,
                    schema: ImportSchema::new(vec![ImportColumn::new(
                        "email",
                        "email",
                        CellType::Text,
                    )])?,
                },
                &tenant_options,
                persist_rows,
            ),
        )
        .await??;
        assert_eq!(tenant_report.rows_committed, 2);
    }
    let db = app.state().resolve::<Db>().await?;
    let rows = db
        .connection()?
        .query_one_raw(Statement::from_string(
            db.backend()?,
            "SELECT COUNT(*) AS row_count FROM exchange_task_rows".to_owned(),
        ))
        .await?
        .ok_or("missing row count")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(
        rows, 11,
        "completed batches must not be applied twice on replay"
    );

    let output = StoragePath::new("private/exports/customers.csv")?;
    let export_attempt = context(app, "export")?.begin_attempt()?;
    let export_result = export_storage_task(
        export_attempt.context(),
        "local",
        output.clone(),
        ExchangeFormat::Csv,
        vec!["email".to_owned()],
        &ExportTaskOptions {
            writer: ExportOptions::default(),
            ..ExportTaskOptions::default()
        },
        |sink| async move {
            let mut row = Map::new();
            row.insert("email".to_owned(), json!("exported@example.test"));
            sink.send(row).await
        },
    )
    .await;
    let export = finish_attempt(
        export_result,
        export_attempt
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await,
    )?;
    assert_eq!(export.rows_written, 1);
    let published_output = StoragePath::new(export.path.clone())?;
    assert_ne!(published_output, output);
    let retry_context = JobContext::new("export", 2, 3, Cancellation::new())?;
    assert_ne!(
        published_output,
        ExportTaskOptions::default().output_path_for_attempt(&output, retry_context.id(), 2)?
    );
    let mut reader = storage
        .disk("local")
        .await?
        .open(&published_output, None)
        .await?;
    let mut exported = Vec::new();
    while let Some(chunk) = reader.body.next().await {
        exported.extend_from_slice(&chunk?);
    }
    assert!(String::from_utf8(exported)?.contains("exported@example.test"));

    let locks = SharedLocks::new((*db).clone());
    let mut lock_hash = Sha256::new();
    lock_hash.update(b"local");
    lock_hash.update([0]);
    lock_hash.update(published_output.as_str().as_bytes());
    let mut lock_suffix = String::with_capacity(64);
    for byte in lock_hash.finalize() {
        write!(&mut lock_suffix, "{byte:02x}")?;
    }
    let lock_key = format!("openrust.data-exchange.object.{lock_suffix}");
    let lock_lease = match locks.acquire(&lock_key, Duration::from_secs(30)).await? {
        openrust::database::LockAcquire::Acquired(lease) => lease,
        openrust::database::LockAcquire::Busy { .. } => {
            return Err("unexpected busy object lock".into());
        }
    };
    let cleanup_attempt = context(app, "locked-cleanup")?.begin_attempt()?;
    let cleanup_result = ImportReportCleanupJob {
        marker_format: 2,
        disk: "local".to_owned(),
        path: published_output.as_str().to_owned(),
        expires_at_unix_secs: 0,
    }
    .handle(cleanup_attempt.context().clone())
    .await;
    let cleanup_closed = cleanup_attempt
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    let cleanup_error =
        cleanup_result.expect_err("cleanup must not pass an active object writer lock");
    assert_eq!(cleanup_error.code(), "data_exchange_object_busy");
    cleanup_closed?;
    locks.release(&lock_lease).await?;
    assert!(
        storage
            .disk("local")
            .await?
            .stat(&published_output)
            .await
            .is_ok()
    );

    queue
        .dispatch_with_options(
            "data-exchange-cleanup",
            ImportReportCleanupJob::NAME,
            &ImportReportCleanupJob {
                marker_format: 2,
                disk: "local".to_owned(),
                path: published_output.as_str().to_owned(),
                expires_at_unix_secs: 0,
            },
            DispatchOptions::default(),
        )
        .await?;
    assert!(
        queue
            .run_next_with_state("data-exchange-cleanup", app.state())
            .await?
    );
    assert!(
        storage
            .disk("local")
            .await?
            .stat(&published_output)
            .await
            .is_ok()
    );

    let cancellation = Cancellation::new();
    cancellation.cancel();
    let cancelled_attempt = JobContext::new("export-cancelled", 1, 3, cancellation)?
        .with_app_state(app.state().clone())
        .begin_attempt()?;
    let cancelled_path = StoragePath::new("private/exports/cancelled.csv")?;
    let cancelled_result = export_storage_task(
        cancelled_attempt.context(),
        "local",
        cancelled_path.clone(),
        ExchangeFormat::Csv,
        vec!["email".to_owned()],
        &ExportTaskOptions::default(),
        |_sink| std::future::pending::<FrameworkResult<()>>(),
    )
    .await;
    let cancelled_closed = cancelled_attempt
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    assert!(
        cancelled_result.is_err(),
        "cancelled export task must fail independently of cleanup"
    );
    cancelled_closed?;
    assert!(
        storage
            .disk("local")
            .await?
            .stat(&cancelled_path)
            .await
            .is_err()
    );

    let cleanup_path = StoragePath::new("private/reports/cleanup-probe.json")?;
    put_source(app, &cleanup_path, b"temporary report").await?;
    queue
        .dispatch_with_options(
            "data-exchange-cleanup",
            ImportReportCleanupJob::NAME,
            &ImportReportCleanupJob {
                marker_format: 2,
                disk: "local".to_owned(),
                path: cleanup_path.as_str().to_owned(),
                expires_at_unix_secs: 0,
            },
            DispatchOptions::default(),
        )
        .await?;
    assert!(
        queue
            .run_next_with_state("data-exchange-cleanup", app.state())
            .await?
    );
    assert!(
        storage
            .disk("local")
            .await?
            .stat(&cleanup_path)
            .await
            .is_err()
    );

    let legacy_path = StoragePath::new("private/reports/legacy-cleanup.json")?;
    put_source(app, &legacy_path, b"retained report").await?;
    let mut marker_hash = Sha256::new();
    marker_hash.update(b"local");
    marker_hash.update([0]);
    marker_hash.update(legacy_path.as_str().as_bytes());
    let mut marker_suffix = String::new();
    for byte in marker_hash.finalize() {
        write!(&mut marker_suffix, "{byte:02x}")?;
    }
    let legacy_marker =
        StoragePath::new(format!(".openrust-data-exchange-retention/{marker_suffix}"))?;
    put_source(
        app,
        &legacy_marker,
        b"{\"expires_at_unix_secs\":4102444800}",
    )
    .await?;
    let legacy: ImportReportCleanupJob = serde_json::from_value(json!({
        "disk": "local",
        "path": legacy_path.as_str(),
        "expires_at_unix_secs": 0
    }))?;
    let legacy_attempt = context(app, "legacy-cleanup")?.begin_attempt()?;
    let legacy_result = legacy.handle(legacy_attempt.context().clone()).await;
    legacy_attempt
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    assert_eq!(
        legacy_result
            .expect_err("legacy cleanup must not delete")
            .code(),
        "data_exchange_cleanup_format_unsupported"
    );
    let disk = storage.disk("local").await?;
    assert!(disk.stat(&legacy_path).await.is_ok());
    assert!(disk.stat(&legacy_marker).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn import_batches_are_atomic_idempotent_and_export_is_streamed()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping data exchange task integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let (app, database, _storage_root) = build_app(&admin_url).await?;
    let outcome = std::panic::AssertUnwindSafe(run_data_exchange_flow(&app))
        .catch_unwind()
        .await;
    let shutdown = std::panic::AssertUnwindSafe(
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)),
    )
    .catch_unwind()
    .await;
    let closed = std::panic::AssertUnwindSafe(database.close())
        .catch_unwind()
        .await;
    let cleanup = match (shutdown, closed) {
        (Ok(Ok(())), Ok(Ok(()))) => Ok(Ok(())),
        (Err(payload), _) | (_, Err(payload)) => Err(payload),
        (Ok(Err(error)), _) | (_, Ok(Err(error))) => {
            Ok(Err(Box::<dyn std::error::Error>::from(error)))
        }
    };
    match outcome {
        Ok(Ok(())) => match cleanup {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        },
        Ok(Err(error)) => {
            if !matches!(cleanup, Ok(Ok(()))) {
                eprintln!("STV_DATA_EXCHANGE_CLEANUP secondary_failure");
            }
            Err(error)
        }
        Err(payload) => {
            if !matches!(cleanup, Ok(Ok(()))) {
                eprintln!("STV_DATA_EXCHANGE_CLEANUP secondary_failure");
            }
            std::panic::resume_unwind(payload)
        }
    }
}
