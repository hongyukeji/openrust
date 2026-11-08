//! 文章文件交换复用框架格式边界和唯一写入服务；不导出密文或私人备注。

use crate::modules::example::{
    http::requests::{
        article::ArticleInput,
        article_transfer::{
            ArticleFormat, MAX_TRANSFER_BYTES, MAX_TRANSFER_ROWS, invalid, too_large,
        },
    },
    models::article,
    services::article_write::ArticleWriter,
};
use openrust::data::exchange::{
    CellType, CsvFormulaPolicy, ExportLimits, ExportOptions, ImportColumn, ImportLimits,
    ImportSchema, XlsxImportOptions, for_each_csv, for_each_xlsx, typed_validator, write_csv,
    write_xlsx,
};
use openrust::database::sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use openrust::foundation::RegisterContext;
use openrust::support::ServiceProvider;
use openrust::{Db, Error, Result};
use serde_json::{Map, Value};
use std::{io::Cursor, sync::Arc};

pub(crate) struct ArticleTransferProvider;
impl ServiceProvider for ArticleTransferProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<ArticleTransfer, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(ArticleTransfer::new(
                        resolver.resolve::<Db>().await?,
                        resolver.resolve::<ArticleWriter>().await?,
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }
    #[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.resolve::<openrust::queue::QueueContributions>()
                .await?
                .register_job::<ArticleExportJob>()?;
            let contributions = app.resolve::<openrust::queue::QueueContributions>().await?;
            contributions.register_job::<ArticleImportJob>()?;
            contributions.register_job::<tasks::ArticleImportSourceCleanupJob>()?;
            app.command(ReconcileImportSources);
            Ok(())
        })
    }
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
struct ReconcileImportSources;

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
impl openrust::console::Command for ReconcileImportSources {
    fn name(&self) -> &'static str {
        "example imports reconcile"
    }

    fn about(&self) -> &'static str {
        "Retry bounded example article import source cleanup"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("[cursor]")
    }

    fn accepts_args(&self) -> bool {
        true
    }

    #[allow(clippy::print_stdout)] // 显式维护命令向调用者报告安全计数与下一页游标。
    fn run<'a>(
        &'a self,
        app: &'a openrust::foundation::Application,
        args: Vec<String>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let after = match args.as_slice() {
                [] => None,
                [cursor] => Some(cursor.as_str()),
                _ => {
                    return Err(invalid(
                        "example_import_reconcile_arguments",
                        "恢复命令最多接受一个游标",
                    ));
                }
            };
            let db = app.state().resolve::<Db>().await?;
            let storage = app
                .state()
                .resolve::<openrust::filesystem::FilesystemManager>()
                .await?;
            let queue = app.state().resolve::<openrust::queue::Queue>().await?;
            let result = tasks::reconcile_imports(db, storage, queue, after).await?;
            println!(
                "checked {} example import source receipts; failed {}; next cursor {}",
                result.checked,
                result.failed,
                result.next.as_deref().unwrap_or("none")
            );
            if result.failed != 0 {
                return Err(invalid(
                    "example_import_reconcile_incomplete",
                    "部分导入来源尚未清理",
                ));
            }
            Ok(())
        })
    }
}

pub(crate) struct ArticleTransfer {
    db: Arc<Db>,
    writer: Arc<ArticleWriter>,
}
#[derive(serde::Serialize)]
pub(crate) struct TransferIssue {
    pub(crate) row: u64,
    pub(crate) field: Option<&'static str>,
    pub(crate) code: &'static str,
}
#[derive(serde::Serialize)]
pub(crate) struct TransferReport {
    pub(crate) rows_read: u64,
    pub(crate) rows_created: u64,
    pub(crate) rows_rejected: u64,
    pub(crate) errors_truncated: bool,
    pub(crate) issues: Vec<TransferIssue>,
}
struct PreparedImport {
    rows: Vec<(u64, ArticleInput)>,
    report: TransferReport,
}

impl ArticleTransfer {
    pub(crate) fn new(db: Arc<Db>, writer: Arc<ArticleWriter>) -> Self {
        Self { db, writer }
    }

    /// 先完成有界解析再写入；每行独立提交，取消或后续失败不撤销前序成功行。
    pub(crate) async fn import(
        &self,
        owner_id: i64,
        format: ArticleFormat,
        bytes: Vec<u8>,
    ) -> Result<TransferReport> {
        authorize(owner_id)?;
        let prepared = tokio::task::spawn_blocking(move || prepare_import(format, bytes))
            .await
            .map_err(|_| invalid("example_transfer_parse_failed", "文件解析未完成"))??;
        let mut report = prepared.report;
        for (row, input) in prepared.rows {
            match self.writer.create(owner_id, input).await {
                Ok(_) => report.rows_created += 1,
                Err(_) => report.issues.push(TransferIssue {
                    row,
                    field: None,
                    code: "article_create_failed",
                }),
            }
        }
        report.rows_rejected = report.rows_read.saturating_sub(report.rows_created);
        report.issues.sort_by_key(|issue| issue.row);
        Ok(report)
    }
    pub(crate) async fn export(&self, owner_id: i64, format: ArticleFormat) -> Result<Vec<u8>> {
        let rows = authorized_rows(&self.db, owner_id).await?;
        tokio::task::spawn_blocking(move || encode_export(format, rows))
            .await
            .map_err(|_| invalid("example_transfer_export_failed", "文件导出未完成"))?
    }
}

pub(crate) fn import_limits() -> ImportLimits {
    ImportLimits {
        max_input_bytes: MAX_TRANSFER_BYTES as u64,
        max_rows: MAX_TRANSFER_ROWS,
        max_columns: 8,
        max_cell_bytes: 40_000,
        max_errors_per_row: 4,
        max_errors: 200,
        max_archive_bytes: 8 * 1024 * 1024,
        max_archive_entries: 64,
        max_shared_strings_bytes: 2 * 1024 * 1024,
        max_shared_string_count: 1000,
    }
}
fn schema() -> Result<ImportSchema> {
    Ok(ImportSchema::new(vec![
        ImportColumn::new("title", "title", CellType::Text),
        ImportColumn::new("body", "body", CellType::Text),
    ])?
    .with_validator(typed_validator::<ArticleInput>()))
}
fn prepare_import(format: ArticleFormat, bytes: Vec<u8>) -> Result<PreparedImport> {
    if bytes.len() > MAX_TRANSFER_BYTES {
        return Err(too_large());
    }
    let mut rows = Vec::new();
    let accept = |row: openrust::data::exchange::ImportRow| {
        if row.errors.is_empty() {
            let input = serde_json::from_value::<ArticleInput>(Value::Object(row.values))
                .map_err(|_| invalid("example_transfer_row_invalid", "文章字段映射失败"))?;
            rows.push((row.row_number, input));
        }
        Ok(())
    };
    let report = match format {
        ArticleFormat::Csv => {
            for_each_csv(Cursor::new(bytes), &schema()?, &import_limits(), accept)
        }
        ArticleFormat::Xlsx => for_each_xlsx(
            Cursor::new(bytes),
            &XlsxImportOptions::default(),
            &schema()?,
            &import_limits(),
            accept,
        ),
    }
    .map_err(|_| {
        invalid(
            "example_transfer_file_invalid",
            "文件格式、字段或资源预算不符合要求",
        )
    })?;
    let issues = report
        .errors
        .into_iter()
        .map(|error| TransferIssue {
            row: error.row_number,
            field: match error.field.as_deref() {
                Some("title") => Some("title"),
                Some("body") => Some("body"),
                _ => None,
            },
            code: "article_field_invalid",
        })
        .collect();
    Ok(PreparedImport {
        rows,
        report: TransferReport {
            rows_read: report.rows_read,
            rows_created: 0,
            rows_rejected: report.rows_invalid,
            errors_truncated: report.errors_truncated,
            issues,
        },
    })
}
fn authorize(owner_id: i64) -> Result<()> {
    if owner_id <= 0 {
        return Err(Error::Unauthorized);
    }
    Ok(())
}
async fn authorized_rows(db: &Db, owner_id: i64) -> Result<Vec<Map<String, Value>>> {
    authorize(owner_id)?;
    let models = article::Entity::find()
        .filter(article::Column::OwnerId.eq(owner_id))
        .order_by_asc(article::Column::Id)
        .limit(MAX_TRANSFER_ROWS + 1)
        .all(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?;
    if u64::try_from(models.len()).map_err(Error::internal)? > MAX_TRANSFER_ROWS {
        return Err(invalid(
            "example_transfer_rows_exceeded",
            "请缩小导出范围，最多导出100篇文章",
        ));
    }
    Ok(models.into_iter().map(public_row).collect())
}
fn public_row(model: article::Model) -> Map<String, Value> {
    // 使用白名单而非序列化整个 Model，避免新增私有列被意外带出。
    Map::from_iter([
        ("title".into(), Value::from(model.title)),
        ("body".into(), Value::from(model.body)),
    ])
}
fn headers() -> Vec<String> {
    ["title", "body"].into_iter().map(str::to_owned).collect()
}
fn export_options() -> ExportOptions {
    ExportOptions {
        limits: ExportLimits {
            max_rows: MAX_TRANSFER_ROWS,
            max_columns: 2,
            max_cell_bytes: 40_000,
            max_output_bytes: 2 * 1024 * 1024,
        },
        csv_formula_policy: CsvFormulaPolicy::Escape,
    }
}
pub(crate) fn encode_export(
    format: ArticleFormat,
    rows: Vec<Map<String, Value>>,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    match format {
        ArticleFormat::Csv => write_csv(
            &mut bytes,
            &headers(),
            rows.into_iter().map(Ok),
            &export_options(),
        )?,
        ArticleFormat::Xlsx => write_xlsx(
            &mut bytes,
            "Articles",
            &headers(),
            rows.into_iter().map(Ok),
            &export_options(),
        )?,
    }
    Ok(bytes)
}

#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
mod tasks {
    #[allow(clippy::wildcard_imports)] // 子模块复用同文件的私有业务 helper 与界限常量。
    use super::*;
    use openrust::container::attributes::Storage;
    use openrust::container::{ClassAttribute, ClassDescriptor, ConstructorParam};
    use openrust::data::exchange::tasks::{ExchangeFormat, ExportTaskOptions, export_storage_task};
    use openrust::filesystem::StoragePath;
    use openrust::filesystem::StorageReader;
    use openrust::filesystem::{FilesystemAdapter, FilesystemManager};
    use openrust::queue::{Job, JobContext, Queue};

    /// 只由认证 HTTP 入口构造；owner 不接受请求 JSON 覆盖。
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    pub(crate) struct ArticleExportJob {
        pub(crate) owner_id: i64,
        pub(crate) format: ArticleFormat,
    }
    impl Job for ArticleExportJob {
        const NAME: &'static str = "example_article_export.v1";
        fn handle(&self, context: JobContext) -> openrust::BoxFuture<'_, Result<()>> {
            Box::pin(async move {
                let db = context.service::<Db>().await?;
                let rows = authorized_rows(&db, self.owner_id).await?;
                export_storage_task(
                    &context,
                    "local",
                    base_path(self.owner_id, self.format)?,
                    task_format(self.format),
                    headers(),
                    &options(),
                    move |sink| async move {
                        for row in rows {
                            sink.send(row).await?;
                        }
                        Ok(())
                    },
                )
                .await?;
                Ok(())
            })
        }
    }
    /// 来源只接受服务器生成的 UUID，重试复用同一来源和业务语义。
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    pub(crate) struct ArticleImportJob {
        pub(crate) owner_id: i64,
        pub(crate) import_id: String,
        pub(crate) format: ArticleFormat,
    }
    impl Job for ArticleImportJob {
        const NAME: &'static str = "example_article_import.v1";
        #[allow(clippy::too_many_lines)] // 租约、事务批次与终态记录在一个 Job attempt 内顺序收束。
        fn handle(&self, context: JobContext) -> openrust::BoxFuture<'_, Result<()>> {
            Box::pin(async move {
                let db = context.service::<Db>().await?;
                with_import_lease(
                    Arc::clone(&db),
                    self.owner_id,
                    &self.import_id,
                    |locks, lease| async move {
                        let receipt = receipt(&db, self.owner_id, &self.import_id).await?;
                        if receipt.format != format_name(self.format) {
                            return Err(binding_error());
                        }
                        match receipt.state_case().map_err(|_| binding_error())? {
                            article_import::ImportState::Completed => return Ok(()),
                            article_import::ImportState::Failed => return Err(closed_error()),
                            article_import::ImportState::Open => {}
                        }
                        if expired(receipt.created_at_ms, now_ms()?) {
                            terminal(
                                &db,
                                &locks,
                                &lease,
                                receipt,
                                "failed",
                                Some("example_import_expired"),
                            )
                            .await?;
                            schedule_cleanup(
                                context.service::<Queue>().await?.as_ref(),
                                self.owner_id,
                                &self.import_id,
                                0,
                                0,
                            )
                            .await?;
                            return Err(expired_error());
                        }
                        let writer = context.service::<ArticleWriter>().await?;
                        let owner_id = self.owner_id;
                        let created = receipt.created_at_ms;
                        let batch_locks = locks.clone();
                        let batch_lease = lease.clone();
                        let input = openrust::data::exchange::tasks::ImportTaskInput {
                            disk: "local".into(),
                            path: import_source(owner_id, &self.import_id)?,
                            version: None,
                            import_id: format!("article-{owner_id}-{}", self.import_id),
                            format: task_format(self.format),
                            schema: schema()?,
                        };
                        let result = openrust::data::exchange::tasks::import_storage_task(
                            &context,
                            input,
                            &import_options(owner_id, &self.import_id)?,
                            move |transaction, rows| {
                                let writer = Arc::clone(&writer);
                                let locks = batch_locks.clone();
                                let lease = batch_lease.clone();
                                Box::pin(async move {
                                    locks
                                        .assert_current_in_transaction(transaction, &lease)
                                        .await?;
                                    ensure_fresh(created)?;
                                    for row in rows {
                                        let input = serde_json::from_value::<ArticleInput>(
                                            Value::Object(row),
                                        )
                                        .map_err(|_| {
                                            invalid(
                                                "example_transfer_row_invalid",
                                                "文章字段映射失败",
                                            )
                                        })?;
                                        writer
                                            .create_in_transaction(transaction, owner_id, input)
                                            .await?;
                                    }
                                    ensure_fresh(created)?;
                                    locks
                                        .assert_current_in_transaction(transaction, &lease)
                                        .await?;
                                    Ok(())
                                })
                            },
                        )
                        .await;
                        let terminal_failure = result.as_ref().err().is_some_and(|error| {
                            error.code() == "example_import_expired"
                                || (context.attempt() >= context.max_attempts()
                                    && !context.is_cancelled()
                                    && error.code() != "queue_job_cancelled")
                        });
                        if result.is_ok() || terminal_failure {
                            let success = result.is_ok();
                            terminal(
                                &db,
                                &locks,
                                &lease,
                                receipt,
                                if success { "completed" } else { "failed" },
                                if success {
                                    None
                                } else if result
                                    .as_ref()
                                    .err()
                                    .is_some_and(|error| error.code() == "example_import_expired")
                                {
                                    Some("example_import_expired")
                                } else {
                                    Some("example_import_failed")
                                },
                            )
                            .await?;
                            schedule_cleanup(
                                context.service::<Queue>().await?.as_ref(),
                                owner_id,
                                &self.import_id,
                                0,
                                0,
                            )
                            .await?;
                        }
                        result.map(|_| ())
                    },
                )
                .await
            })
        }
    }

    use crate::modules::example::models::article_import;
    use openrust::database::sea_orm::{ActiveModelTrait, ActiveValue::Set, IntoActiveModel};
    use openrust::database::{LockAcquire, LockLease, SharedLocks};
    const IMPORT_WINDOW_MS: i64 = 24 * 3600 * 1000;
    const LEASE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

    fn now_ms() -> Result<i64> {
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid("example_import_clock_invalid", "导入时钟不可用"))?;
        i64::try_from(elapsed.as_millis())
            .map_err(|_| invalid("example_import_clock_invalid", "导入时钟不可用"))
    }
    fn expired(created: i64, now: i64) -> bool {
        now < created || now.saturating_sub(created) >= IMPORT_WINDOW_MS
    }
    fn ensure_fresh(created: i64) -> Result<()> {
        if expired(created, now_ms()?) {
            Err(expired_error())
        } else {
            Ok(())
        }
    }
    fn expired_error() -> Error {
        invalid(
            "example_import_expired",
            "导入已超过24小时恢复窗口，请创建新导入",
        )
    }
    fn closed_error() -> Error {
        invalid(
            "example_import_closed",
            "导入已终止，请查看状态并创建新导入",
        )
    }
    fn binding_error() -> Error {
        invalid(
            "example_import_binding_invalid",
            "导入与业务账户或来源不匹配",
        )
    }
    fn format_name(format: ArticleFormat) -> &'static str {
        match format {
            ArticleFormat::Csv => "csv",
            ArticleFormat::Xlsx => "xlsx",
        }
    }
    fn lock_key(owner: i64, id: &str) -> String {
        format!("example:article-import:{owner}:{id}")
    }
    async fn receipt(db: &Db, owner: i64, id: &str) -> Result<article_import::Model> {
        let source = import_source(owner, id)?;
        let row = article_import::Entity::find_by_id(id)
            .one(&db.primary_connection()?)
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::NotFound("import not found".into()))?;
        if row.owner_id != owner {
            return Err(Error::NotFound("import not found".into()));
        }
        if row.source_key != source.as_str() {
            return Err(binding_error());
        }
        Ok(row)
    }
    pub(in crate::modules::example) async fn terminal(
        db: &Db,
        locks: &SharedLocks,
        lease: &LockLease,
        row: article_import::Model,
        state: &'static str,
        error: Option<&'static str>,
    ) -> Result<()> {
        let locks = locks.clone();
        let lease = lease.clone();
        let now = now_ms()?;
        db.transaction(move |transaction| {
            Box::pin(async move {
                locks
                    .assert_current_in_transaction(transaction, &lease)
                    .await?;
                let mut row = row.into_active_model();
                row.state = Set(article_import::state_for_storage(state).await?);
                row.error_code = Set(error.map(str::to_owned));
                row.terminal_at_ms = Set(Some(now));
                row.update(transaction).await.map_err(Error::internal)?;
                Ok(())
            })
        })
        .await
    }
    pub(in crate::modules::example) async fn with_import_lease<T, F, Fut>(
        db: Arc<Db>,
        owner: i64,
        id: &str,
        operation: F,
    ) -> Result<T>
    where
        T: Send,
        F: FnOnce(SharedLocks, LockLease) -> Fut + Send,
        Fut: std::future::Future<Output = Result<T>> + Send,
    {
        import_source(owner, id)?;
        let locks = SharedLocks::new((*db).clone());
        let lease = match locks.acquire(&lock_key(owner, id), LEASE_TTL).await? {
            LockAcquire::Acquired(lease) => lease,
            LockAcquire::Busy { .. } => {
                return Err(invalid("example_import_busy", "导入来源正在使用或清理"));
            }
        };
        let result = {
            let future = operation(locks.clone(), lease.clone());
            tokio::pin!(future);
            let mut ticks = tokio::time::interval(std::time::Duration::from_secs(30));
            ticks.tick().await;
            loop {
                tokio::select! {
                    result = &mut future => break result,
                    _ = ticks.tick() => {
                        // 同时推进回调，避免单连接池中续租等待事务、事务又等待本地 poll。
                        tokio::select! {
                            result = &mut future => break result,
                            renewal = tokio::time::timeout(std::time::Duration::from_secs(30),locks.renew(&lease,LEASE_TTL)) => {
                                if !matches!(renewal,Ok(Ok(true))) { break Err(invalid("example_import_lease_lost", "导入租约丢失，等待安全重试")); }
                            }
                        }
                    }
                }
            }
        };
        // operation 已在上面的作用域内释放，再释放自己的租约。
        let released = locks.release(&lease).await;
        if !matches!(released, Ok(true)) && result.is_ok() {
            return Err(invalid(
                "example_import_lease_lost",
                "导入租约丢失，等待安全重试",
            ));
        }
        result
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    pub(crate) struct ArticleImportSourceCleanupJob {
        owner_id: i64,
        import_id: String,
        #[serde(default)]
        deferrals: u8,
    }
    impl Job for ArticleImportSourceCleanupJob {
        const NAME: &'static str = "example_article_import_source_cleanup.v1";
        fn handle(&self, context: JobContext) -> openrust::BoxFuture<'_, Result<()>> {
            Box::pin(async move {
                cleanup_import_at(
                    context,
                    self.owner_id,
                    &self.import_id,
                    self.deferrals,
                    now_ms()?,
                )
                .await
            })
        }
    }
    async fn schedule_cleanup(
        queue: &Queue,
        owner: i64,
        id: &str,
        deferrals: u8,
        delay: u64,
    ) -> Result<()> {
        if deferrals > 8 {
            return Err(invalid(
                "example_import_cleanup_deferred",
                "导入尚未结束，来源保留待检查",
            ));
        }
        queue
            .dispatch_with_options(
                "example-import-cleanup",
                ArticleImportSourceCleanupJob::NAME,
                &ArticleImportSourceCleanupJob {
                    owner_id: owner,
                    import_id: id.into(),
                    deferrals,
                },
                openrust::queue::DispatchOptions {
                    delay: std::time::Duration::from_secs(delay),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }
    pub(crate) async fn cleanup_import_at(
        context: JobContext,
        owner: i64,
        id: &str,
        deferrals: u8,
        now: i64,
    ) -> Result<()> {
        let db = context.service::<Db>().await?;
        let queue = context.service::<Queue>().await?;
        let storage = context.service::<FilesystemManager>().await?;
        cleanup_import_with_services(db, storage, queue, owner, id, deferrals, now).await
    }

    async fn cleanup_import_with_services(
        db: Arc<Db>,
        storage: Arc<FilesystemManager>,
        queue: Arc<Queue>,
        owner: i64,
        id: &str,
        deferrals: u8,
        now: i64,
    ) -> Result<()> {
        let operation_db = Arc::clone(&db);
        let operation_queue = Arc::clone(&queue);
        let result = with_import_lease(Arc::clone(&db), owner, id, |locks, lease| async move {
            let db = operation_db;
            let queue = operation_queue;
            let mut row = receipt(&db, owner, id).await?;
            if row.state == "open" && expired(row.created_at_ms, now) {
                terminal(
                    &db,
                    &locks,
                    &lease,
                    row,
                    "failed",
                    Some("example_import_expired"),
                )
                .await?;
                row = receipt(&db, owner, id).await?;
            }
            if row.state == "open" {
                return schedule_cleanup(&queue, owner, id, deferrals.saturating_add(1), 24 * 3600)
                    .await;
            }
            if !matches!(row.state.as_str(), "completed" | "failed") {
                return Err(binding_error());
            }
            if row.source_removed {
                return Ok(());
            }
            match storage
                .disk("local")
                .await?
                .delete(&import_source(owner, id)?)
                .await
            {
                Ok(()) => {}
                Err(error) if error.code() == "storage_object_not_found" => {}
                Err(error) => return Err(error),
            }
            let mut active = row.into_active_model();
            active.source_removed = Set(true);
            db.transaction(move |transaction| {
                Box::pin(async move {
                    locks
                        .assert_current_in_transaction(transaction, &lease)
                        .await?;
                    active.update(transaction).await.map_err(Error::internal)?;
                    Ok(())
                })
            })
            .await
        })
        .await;
        match result {
            Err(error) if error.code() == "example_import_busy" => {
                schedule_cleanup(&queue, owner, id, deferrals.saturating_add(1), 300).await
            }
            result => result,
        }
    }

    /// 单次最多检查 100 条未回收收据；重复执行可继续处理剩余记录。
    pub(crate) struct ReconcileBatch {
        pub(crate) checked: usize,
        pub(crate) failed: usize,
        pub(crate) next: Option<String>,
    }

    pub(crate) async fn reconcile_imports(
        db: Arc<Db>,
        storage: Arc<FilesystemManager>,
        queue: Arc<Queue>,
        after: Option<&str>,
    ) -> Result<ReconcileBatch> {
        if let Some(after) = after {
            uuid::Uuid::parse_str(after)
                .map_err(|_| invalid("example_import_reconcile_cursor", "恢复游标无效"))?;
        }
        let mut query =
            article_import::Entity::find().filter(article_import::Column::SourceRemoved.eq(false));
        if let Some(after) = after {
            query = query.filter(article_import::Column::ImportId.gt(after.to_owned()));
        }
        let pending = query
            .order_by_asc(article_import::Column::ImportId)
            .limit(100)
            .all(&db.primary_connection()?)
            .await
            .map_err(Error::internal)?;
        let now = now_ms()?;
        let mut checked = 0;
        let mut failed = 0;
        let mut next = None;
        for row in pending {
            let result = cleanup_import_with_services(
                Arc::clone(&db),
                Arc::clone(&storage),
                Arc::clone(&queue),
                row.owner_id,
                &row.import_id,
                0,
                now,
            )
            .await;
            checked += 1;
            failed += usize::from(result.is_err());
            next = Some(row.import_id);
        }
        Ok(ReconcileBatch {
            checked,
            failed,
            next,
        })
    }
    #[derive(serde::Serialize)]
    pub(crate) struct ImportStatus {
        state: String,
        error_code: Option<String>,
        source_removed: bool,
    }
    pub(crate) async fn import_status(db: &Db, owner: i64, id: &str) -> Result<ImportStatus> {
        let row = receipt(db, owner, id).await?;
        Ok(ImportStatus {
            state: row.state,
            error_code: row.error_code,
            source_removed: row.source_removed,
        })
    }
    fn import_source(owner_id: i64, import_id: &str) -> Result<StoragePath> {
        authorize(owner_id)?;
        let id = uuid::Uuid::parse_str(import_id)
            .map_err(|_| invalid("example_transfer_import_invalid", "导入标识无效"))?;
        if id.to_string() != import_id {
            return Err(invalid("example_transfer_import_invalid", "导入标识无效"));
        }
        StoragePath::new(format!("example/articles/{owner_id}/imports/{id}/source"))
    }
    fn import_options(
        owner_id: i64,
        import_id: &str,
    ) -> Result<openrust::data::exchange::tasks::ImportTaskOptions> {
        let source = import_source(owner_id, import_id)?;
        Ok(openrust::data::exchange::tasks::ImportTaskOptions {
            batch_size: 2,
            max_batch_bytes: 128 * 1024,
            limits: import_limits(),
            report_disk: "local".into(),
            report_path: StoragePath::new(format!("{}.report", source.as_str()))?,
            report_retention: std::time::Duration::from_secs(3600),
        })
    }
    #[derive(serde::Serialize)]
    pub(crate) struct ImportQueued {
        pub(crate) import_id: String,
        pub(crate) job_id: String,
    }

    pub(crate) struct ArticleImportDisk(Arc<FilesystemAdapter>);

    pub(crate) fn article_import_disk_descriptor() -> ClassDescriptor {
        ClassDescriptor::concrete::<ArticleImportDisk>(
            "Modules/Example/ArticleImportDisk",
            vec![
                ConstructorParam::class::<FilesystemAdapter>("disk")
                    .with_attribute(ClassAttribute::contextual(|| Ok(Storage::new("local")))),
            ],
            |args| {
                Ok(Arc::new(ArticleImportDisk(
                    args.service::<FilesystemAdapter>(0)?,
                )))
            },
        )
    }

    pub(crate) async fn upload_import(
        db: &Db,
        import_disk: &ArticleImportDisk,
        queue: &Queue,
        owner_id: i64,
        format: ArticleFormat,
        bytes: Vec<u8>,
    ) -> Result<ImportQueued> {
        authorize(owner_id)?;
        if bytes.len() > MAX_TRANSFER_BYTES {
            return Err(too_large());
        }
        let disk = &import_disk.0;
        let import_id = uuid::Uuid::now_v7().to_string();
        let path = import_source(owner_id, &import_id)?;
        article_import::ActiveModel {
            import_id: Set(import_id.clone()),
            owner_id: Set(owner_id),
            source_key: Set(path.as_str().into()),
            format: Set(format_name(format).into()),
            state: Set(article_import::state_for_storage("open").await?),
            error_code: Set(None),
            created_at_ms: Set(now_ms()?),
            terminal_at_ms: Set(None),
            source_removed: Set(false),
        }
        .insert(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?;
        schedule_cleanup(queue, owner_id, &import_id, 0, 24 * 3600).await?;
        with_import_lease(Arc::new(db.clone()), owner_id, &import_id, |_, _| async {
            ensure_fresh(receipt(db, owner_id, &import_id).await?.created_at_ms)?;
            let chunks = futures_util::stream::unfold(
                openrust::http::axum::body::Bytes::from(bytes),
                |mut bytes| async move {
                    if bytes.is_empty() {
                        None
                    } else {
                        let length = bytes.len().min(16 * 1024);
                        Some((Ok(bytes.split_to(length)), bytes))
                    }
                },
            );
            disk.put_stream(
                path,
                Some(format.mime().into()),
                Box::pin(chunks),
                openrust::filesystem::UploadOptions {
                    visibility: None,
                    max_bytes: MAX_TRANSFER_BYTES as u64,
                    max_chunk_bytes: 16 * 1024,
                    allowed_content_types: vec![format.mime().into()],
                },
                None,
            )
            .await?;
            // Queue 提交结果可能因断连不确定，不能在失败/取消时立即删掉可能已接受的任务来源。
            let job_id = queue
                .dispatch(
                    "example-import",
                    ArticleImportJob::NAME,
                    &ArticleImportJob {
                        owner_id,
                        import_id: import_id.clone(),
                        format,
                    },
                )
                .await?;
            Ok(ImportQueued {
                import_id: import_id.clone(),
                job_id,
            })
        })
        .await
    }
    pub(crate) async fn open_import_report(
        storage: &FilesystemManager,
        owner_id: i64,
        import_id: &str,
        job_id: &str,
        attempt: u64,
    ) -> Result<StorageReader> {
        validate_attempt(job_id, attempt)?;
        let path = import_options(owner_id, import_id)?.report_path_for_attempt(
            job_id,
            attempt,
            &format!("article-{owner_id}-{import_id}"),
        )?;
        storage.disk("local").await?.open(&path, None).await
    }
    fn task_format(format: ArticleFormat) -> ExchangeFormat {
        match format {
            ArticleFormat::Csv => ExchangeFormat::Csv,
            ArticleFormat::Xlsx => ExchangeFormat::Xlsx,
        }
    }
    fn options() -> ExportTaskOptions {
        ExportTaskOptions {
            writer: export_options(),
            retention: std::time::Duration::from_secs(3600),
        }
    }
    fn base_path(owner_id: i64, format: ArticleFormat) -> Result<StoragePath> {
        authorize(owner_id)?;
        let extension = match format {
            ArticleFormat::Csv => "csv",
            ArticleFormat::Xlsx => "xlsx",
        };
        StoragePath::new(format!(
            "example/articles/{owner_id}/exports/articles.{extension}"
        ))
    }
    pub(crate) async fn dispatch(
        queue: &Queue,
        owner_id: i64,
        format: ArticleFormat,
    ) -> Result<String> {
        authorize(owner_id)?;
        queue
            .dispatch(
                "example",
                ArticleExportJob::NAME,
                &ArticleExportJob { owner_id, format },
            )
            .await
    }
    fn validate_attempt(job_id: &str, attempt: u64) -> Result<()> {
        if job_id.is_empty()
            || job_id.len() > 128
            || !job_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !(1..=100).contains(&attempt)
        {
            return Err(invalid(
                "example_transfer_job_invalid",
                "导出任务标识或尝试次数无效",
            ));
        }
        Ok(())
    }
    pub(crate) async fn open(
        storage: &FilesystemManager,
        owner_id: i64,
        format: ArticleFormat,
        job_id: &str,
        attempt: u64,
    ) -> Result<StorageReader> {
        validate_attempt(job_id, attempt)?;
        let path =
            options().output_path_for_attempt(&base_path(owner_id, format)?, job_id, attempt)?;
        storage.disk("local").await?.open(&path, None).await
    }
}
#[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
pub(crate) use tasks::{
    ArticleExportJob, ArticleImportDisk, ArticleImportJob, ImportQueued, ImportStatus,
    article_import_disk_descriptor, dispatch as dispatch_export, import_status,
    open as open_export, open_import_report, upload_import,
};

#[cfg(all(
    test,
    feature = "data-exchange-tasks",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) use tasks::cleanup_import_at;

// SQLite 专项直接复用真实导入的租约与终态事务，不启动 Queue worker。
#[cfg(all(
    test,
    feature = "db-sqlite",
    feature = "data-exchange-tasks",
    feature = "storage-local"
))]
pub(in crate::modules::example) use tasks::{terminal, with_import_lease};
