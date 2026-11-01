//! 基于现有 Queue、Storage、事务与幂等服务的数据交换任务工作流。
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{
    future::Future,
    io::{Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{
    core::{BoxFuture, Error, Result},
    data_exchange::{
        ExportOptions, ImportLimits, ImportRow, ImportSchema, XlsxImportOptions, for_each_csv,
        for_each_xlsx, write_csv, write_xlsx,
    },
    db::{
        DatabaseTransaction, Db, IdempotencyClaim, IdempotencyResponse, LockAcquire,
        RequestIdempotency, SharedLocks,
    },
    queue::{DispatchOptions, Job, JobContext, Queue},
    storage::{Storage, StorageDisk, StoragePath, StorageStream, UploadOptions},
};

const REPORT_ERROR_LIMIT: usize = 32;
const IDEMPOTENCY_TTL: Duration = Duration::from_hours(24);
const IDEMPOTENCY_RETENTION: Duration = Duration::from_hours(24);
const OBJECT_LOCK_TTL: Duration = Duration::from_hours(24);
const OBJECT_LOCK_RENEW_INTERVAL: Duration = Duration::from_secs(300);
const OBJECT_LOCK_RENEW_TIMEOUT: Duration = Duration::from_secs(30);
const CLEANUP_QUEUE: &str = "data-exchange-cleanup";

/// CSV 或 XLSX 输入/输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExchangeFormat {
    /// 逗号分隔值格式。
    Csv,
    /// Office Open XML 表格格式。
    Xlsx,
}

/// 任务化导入的资源与事务配置。
#[derive(Debug, Clone)]
pub struct ImportTaskOptions {
    /// 一次数据库事务最多处理的有效行数，范围为 1–1000。
    pub batch_size: usize,
    /// 一批有效行序列化后的最大估算字节数，范围为 1 KiB–64 MiB。
    pub max_batch_bytes: usize,
    /// 输入格式的上限配置。
    pub limits: ImportLimits,
    /// 错误报告写入的具名存储 disk。
    pub report_disk: String,
    /// 错误报告的应用私有基础路径；框架按任务 ID 和尝试次数生成唯一对象路径。
    pub report_path: StoragePath,
    /// 报告保留时长；到期后通过队列派发清理任务，最多 30 天。
    pub report_retention: Duration,
}

impl ImportTaskOptions {
    /// 返回本次尝试错误报告的唯一 Storage 路径。
    pub fn report_path_for_attempt(
        &self,
        job_id: &str,
        attempt: u64,
        import_id: &str,
    ) -> Result<StoragePath> {
        attempt_object_path(
            &self.report_path,
            "import-report",
            job_id,
            attempt,
            import_id,
        )
    }

    fn validate(&self) -> Result<()> {
        self.limits.validate()?;
        if !(1..=1000).contains(&self.batch_size)
            || !(1024..=64 * 1024 * 1024).contains(&self.max_batch_bytes)
            || self.report_disk.is_empty()
            || self.report_disk.len() > 128
            || self.report_retention.is_zero()
            || self.report_retention > Duration::from_hours(720)
        {
            return Err(task_error(
                "data_exchange_task_options_invalid",
                "导入任务配置超出支持范围",
            ));
        }
        Ok(())
    }
}

/// 授权后的导入对象、格式与字段映射。
#[derive(Clone)]
pub struct ImportTaskInput {
    /// 已授权的源文件 storage disk。
    pub disk: String,
    /// 源文件对象路径。
    pub path: StoragePath,
    /// 后端支持时用于精确绑定源对象版本。
    pub version: Option<String>,
    /// 应用提供的稳定导入标识。
    pub import_id: String,
    /// 源文件格式。
    pub format: ExchangeFormat,
    /// 表头、目标字段和转换器映射。
    pub schema: ImportSchema,
}

/// 已持久化的有界导入摘要；错误消息不包含原始单元格内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTaskReport {
    /// 应用提供的稳定导入 ID。
    pub import_id: String,
    /// 本次尝试报告所在的具名 Storage disk。
    pub report_disk: String,
    /// 本次尝试报告的唯一 Storage 路径。
    pub report_path: String,
    /// 源文件字节数。
    pub source_bytes: u64,
    /// 已解析的数据行数。
    pub rows_read: u64,
    /// 已提交或由幂等记录确认的有效行数。
    pub rows_committed: u64,
    /// 含字段校验错误的行数。
    pub rows_invalid: u64,
    /// 已提交或确认的有效批次数。
    pub batches_committed: u64,
    /// 有界字段错误样本。
    pub errors: Vec<ImportTaskError>,
    /// 错误样本是否因上限而截断。
    pub errors_truncated: bool,
    /// 是否因 worker 合作取消而提前停止。
    pub cancelled: bool,
    /// 解析或持久化失败的稳定错误码；成功或取消时为空。
    pub failure_code: Option<String>,
    /// 报告对象到期的 Unix 秒时间戳。
    pub expires_at_unix_secs: u64,
}

/// 报告中一条脱敏、定长的字段错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTaskError {
    /// 源文件中的 1-based 行号。
    pub row_number: u64,
    /// 应用目标字段；不包含原始单元格值。
    pub field: Option<String>,
    /// 稳定错误码。
    pub code: String,
    /// 截断后的安全说明。
    pub message: String,
}

/// 将 Storage 中的文件解析为有界数据库批次并持久化报告。
///
/// 调用方应在自己的 `Job` 中提供稳定 `import_id`、Schema 与批次写入 callback。
/// callback 仅收到通过转换和校验的 JSON row；框架将每批写入与 `RequestIdempotency`
/// 完成记录放在同一数据库事务中。安装应用 Queue worker 时，任务报告会按配置 TTL
/// 自动派发内置清理 job。请求中必须安装 `CoordinationMigration`。
#[allow(
    clippy::too_many_lines,
    reason = "保持单次导入工作流的事务与报告边界可追踪"
)]
pub async fn import_storage_task<F>(
    context: &JobContext,
    input: ImportTaskInput,
    options: &ImportTaskOptions,
    persist_batch: F,
) -> Result<ImportTaskReport>
where
    F: for<'a> FnMut(&'a DatabaseTransaction, Vec<Map<String, Value>>) -> BoxFuture<'a, Result<()>>
        + Send
        + 'static,
{
    options.validate()?;
    validate_import_id(&input.import_id)?;
    validate_exchange_object_path(&input.path)?;
    validate_exchange_object_path(&options.report_path)?;
    let report_path =
        options.report_path_for_attempt(context.id(), context.attempt(), &input.import_id)?;
    validate_exchange_object_path(&report_path)?;
    let storage = context.service::<Storage>().await?;
    let db = context.service::<Db>().await?;
    let queue = context.service::<Queue>().await?;
    let source = storage.disk(&input.disk).await?;
    let report_disk = storage.disk(&options.report_disk).await?;

    let temporary = tempfile::tempfile().map_err(Error::internal)?;
    let mut temporary = tokio::fs::File::from_std(temporary);
    let mut source_reader = tokio::select! {
        () = context.cancelled() => return Err(task_error("queue_job_cancelled", "导入任务已取消")),
        result = source.open(&input.path, input.version.as_deref()) => result?,
    };
    let mut source_bytes = 0_u64;
    let mut source_hash = Sha256::new();
    let cancellation = context.cancelled();
    tokio::pin!(cancellation);
    loop {
        let chunk = tokio::select! {
            () = &mut cancellation => return Err(task_error("queue_job_cancelled", "导入任务已取消")),
            chunk = source_reader.body.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk?;
        source_bytes = source_bytes.saturating_add(chunk.len() as u64);
        if source_bytes > options.limits.max_input_bytes {
            return Err(task_error(
                "exchange_input_limit_exceeded",
                "输入文件超过配置的字节上限",
            ));
        }
        source_hash.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut temporary, &chunk)
            .await
            .map_err(Error::internal)?;
    }
    tokio::io::AsyncWriteExt::flush(&mut temporary)
        .await
        .map_err(Error::internal)?;
    let mut temporary = temporary.into_std().await;
    temporary
        .seek(SeekFrom::Start(0))
        .map_err(Error::internal)?;
    let source_fingerprint = source_hash.finalize();

    let idempotency = RequestIdempotency::new((*db).clone());
    let (scope, batch_scope) = idempotency_scopes(&input.import_id)?;
    let lease = match idempotency
        .claim(
            &scope,
            &input.import_id,
            &source_fingerprint,
            IDEMPOTENCY_TTL,
        )
        .await?
    {
        IdempotencyClaim::Acquired(lease) => lease,
        IdempotencyClaim::Replay(response) => {
            return serde_json::from_value(response.body().clone()).map_err(Error::internal);
        }
        IdempotencyClaim::InProgress { .. } => {
            return Err(task_error(
                "data_exchange_import_in_progress",
                "相同导入 ID 已有任务正在处理",
            ));
        }
        IdempotencyClaim::Conflict => {
            return Err(task_error(
                "data_exchange_import_conflict",
                "相同导入 ID 已绑定其他输入文件",
            ));
        }
    };

    let mut report = ImportTaskReport {
        import_id: input.import_id.clone(),
        report_disk: options.report_disk.clone(),
        report_path: report_path.as_str().to_owned(),
        source_bytes,
        rows_read: 0,
        rows_committed: 0,
        rows_invalid: 0,
        batches_committed: 0,
        errors: Vec::new(),
        errors_truncated: false,
        cancelled: false,
        failure_code: None,
        expires_at_unix_secs: unix_now().saturating_add(options.report_retention.as_secs()),
    };
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<ImportRow>(1);
    let limits = options.limits.clone();
    let xlsx_options = XlsxImportOptions::default();
    let parser = tokio::task::spawn_blocking(move || {
        let mut temporary = temporary;
        let mut callback = |row| {
            sender
                .blocking_send(row)
                .map_err(|_| task_error("data_exchange_consumer_closed", "导入批次处理已停止"))
        };
        match input.format {
            ExchangeFormat::Csv => {
                for_each_csv(&mut temporary, &input.schema, &limits, &mut callback)
            }
            ExchangeFormat::Xlsx => for_each_xlsx(
                temporary,
                &xlsx_options,
                &input.schema,
                &limits,
                &mut callback,
            ),
        }
    });

    let persist_batch = Arc::new(tokio::sync::Mutex::new(persist_batch));
    let mut batch = Vec::<Map<String, Value>>::new();
    let mut batch_bytes = 0_usize;
    let mut batch_index = 0_u64;
    let mut processing_error = None;
    loop {
        tokio::select! {
            () = &mut cancellation => {
                report.cancelled = true;
                break;
            }
            row = receiver.recv() => {
                let Some(row) = row else { break };
                report.rows_read = report.rows_read.saturating_add(1);
                if !row.errors.is_empty() {
                    report.rows_invalid = report.rows_invalid.saturating_add(1);
                    append_report_errors(&mut report, row);
                    continue;
                }
                let row_bytes = match serde_json::to_vec(&row.values) {
                    Ok(bytes) => bytes.len(),
                    Err(error) => {
                        processing_error = Some(Error::internal(error));
                        break;
                    }
                };
                if row_bytes > options.max_batch_bytes {
                    processing_error = Some(task_error(
                        "data_exchange_batch_limit_exceeded",
                        "单行内容超过事务批次字节上限",
                    ));
                    break;
                }
                if !batch.is_empty()
                    && (batch.len() >= options.batch_size
                        || batch_bytes.saturating_add(row_bytes) > options.max_batch_bytes)
                {
                    let batch_len = batch.len();
                    if let Err(error) = persist_import_batch(
                        &db,
                        &idempotency,
                        &batch_scope,
                        &source_fingerprint,
                        batch_index,
                        std::mem::take(&mut batch),
                        &persist_batch,
                    )
                    .await
                    {
                        processing_error = Some(error);
                        break;
                    }
                    report.rows_committed = report
                        .rows_committed
                        .saturating_add(u64::try_from(batch_len).unwrap_or(0));
                    report.batches_committed = report.batches_committed.saturating_add(1);
                    batch_index = batch_index.saturating_add(1);
                    batch_bytes = 0;
                    if let Err(error) = persist_progress(context, &report).await {
                        if error.code() == "queue_job_cancelled" {
                            report.cancelled = true;
                        } else {
                            processing_error = Some(error);
                        }
                        break;
                    }
                }
                batch_bytes = batch_bytes.saturating_add(row_bytes);
                batch.push(row.values);
            }
        }
    }
    drop(receiver);
    let parsed = parser
        .await
        .map_err(|_| task_error("data_exchange_parser_failed", "数据解析任务未能正常结束"))?;

    if processing_error.is_none()
        && !report.cancelled
        && let Err(error) = parsed
    {
        processing_error = Some(error);
    }
    if context.is_cancelled() {
        report.cancelled = true;
    }
    if processing_error.is_none() && !report.cancelled && !batch.is_empty() {
        let batch_len = batch.len();
        match persist_import_batch(
            &db,
            &idempotency,
            &batch_scope,
            &source_fingerprint,
            batch_index,
            batch,
            &persist_batch,
        )
        .await
        {
            Ok(()) => {
                report.rows_committed = report
                    .rows_committed
                    .saturating_add(u64::try_from(batch_len).unwrap_or(0));
                report.batches_committed = report.batches_committed.saturating_add(1);
                if let Err(error) = persist_progress(context, &report).await {
                    if error.code() == "queue_job_cancelled" {
                        report.cancelled = true;
                    } else {
                        processing_error = Some(error);
                    }
                }
            }
            Err(error) => processing_error = Some(error),
        }
    }
    if let Some(error) = &processing_error {
        report.failure_code = Some(bounded_text(error.code(), 64));
    }
    report.expires_at_unix_secs = unix_now().saturating_add(options.report_retention.as_secs());

    let locks = SharedLocks::new((*db).clone());
    let report_context = context.clone();
    let report_disk_name = options.report_disk.clone();
    let report_path = report_path.clone();
    let report_disk = report_disk.clone();
    let report_queue = queue.clone();
    let report_snapshot = report.clone();
    let report_result = with_object_lock(
        locks,
        report_disk_name.clone(),
        report_path.clone(),
        async move {
            store_report(
                &report_context,
                &report_disk,
                &report_path,
                &report_snapshot,
            )
            .await?;
            write_retention_marker(
                &report_disk,
                &report_disk_name,
                &report_path,
                report_snapshot.expires_at_unix_secs,
            )
            .await?;
            dispatch_report_cleanup(
                &report_queue,
                &report_disk_name,
                &report_path,
                report_snapshot.expires_at_unix_secs,
            )
            .await?;
            Ok(())
        },
    )
    .await;
    if let Err(error) = report_result {
        let _ = idempotency.release(&lease).await;
        return Err(error);
    }

    if report.cancelled || processing_error.is_some() {
        let _ = idempotency.release(&lease).await;
        if let Some(error) = processing_error {
            return Err(error);
        }
        return Err(task_error(
            "queue_job_cancelled",
            "导入已取消；已提交批次保留且取消报告已持久化",
        ));
    }

    let response =
        IdempotencyResponse::new(200, serde_json::to_value(&report).map_err(Error::internal)?)?;
    let completion = db
        .transaction(|transaction| {
            let idempotency = idempotency.clone();
            let lease = lease.clone();
            let response = response.clone();
            Box::pin(async move {
                idempotency
                    .complete_in_transaction(transaction, &lease, &response, IDEMPOTENCY_RETENTION)
                    .await?;
                Ok(())
            })
        })
        .await;
    if let Err(error) = completion {
        let _ = idempotency.release(&lease).await;
        return Err(error);
    }
    Ok(report)
}

async fn persist_import_batch<F>(
    db: &Db,
    idempotency: &RequestIdempotency,
    scope: &str,
    source_fingerprint: &[u8],
    batch_index: u64,
    rows: Vec<Map<String, Value>>,
    persist: &Arc<tokio::sync::Mutex<F>>,
) -> Result<()>
where
    F: for<'a> FnMut(&'a DatabaseTransaction, Vec<Map<String, Value>>) -> BoxFuture<'a, Result<()>>
        + Send
        + 'static,
{
    let mut hasher = Sha256::new();
    hasher.update(source_fingerprint);
    hasher.update(batch_index.to_be_bytes());
    for row in &rows {
        hasher.update(serde_json::to_vec(row).map_err(Error::internal)?);
    }
    let fingerprint = hasher.finalize();
    let key = format!("batch-{batch_index:010}");
    let lease = match idempotency
        .claim(scope, &key, &fingerprint, IDEMPOTENCY_TTL)
        .await?
    {
        IdempotencyClaim::Acquired(lease) => lease,
        IdempotencyClaim::Replay(_) => return Ok(()),
        IdempotencyClaim::InProgress { .. } => {
            return Err(task_error(
                "data_exchange_batch_in_progress",
                "导入批次仍由其他 worker 处理",
            ));
        }
        IdempotencyClaim::Conflict => {
            return Err(task_error(
                "data_exchange_batch_conflict",
                "导入批次内容与已保存的幂等键不一致",
            ));
        }
    };
    let rows_written = u64::try_from(rows.len()).unwrap_or(u64::MAX);
    let response = IdempotencyResponse::new(200, json!({"rows": rows_written}))?;
    let transaction_result = db
        .transaction(|transaction| {
            let lease = lease.clone();
            let response = response.clone();
            let idempotency = idempotency.clone();
            let persist = Arc::clone(persist);
            Box::pin(async move {
                persist.lock().await(transaction, rows).await?;
                idempotency
                    .complete_in_transaction(transaction, &lease, &response, IDEMPOTENCY_RETENTION)
                    .await
            })
        })
        .await;
    if transaction_result.is_err() {
        let _ = idempotency.release(&lease).await;
    }
    transaction_result
}

fn append_report_errors(report: &mut ImportTaskReport, row: ImportRow) {
    for error in row.errors {
        if report.errors.len() >= REPORT_ERROR_LIMIT {
            report.errors_truncated = true;
            break;
        }
        report.errors.push(ImportTaskError {
            row_number: row.row_number,
            field: error.field.map(|field| bounded_text(&field, 128)),
            code: bounded_text(&error.code, 64),
            message: bounded_text(&error.message, 256),
        });
    }
}

async fn persist_progress(context: &JobContext, report: &ImportTaskReport) -> Result<()> {
    context
        .set_progress(&json!({
            "rows_read": report.rows_read,
            "rows_committed": report.rows_committed,
            "rows_invalid": report.rows_invalid,
            "batches_committed": report.batches_committed,
        }))
        .await
}

/// 导出任务可用的有界行写入器。
pub struct ExportRowSink {
    sender: tokio::sync::mpsc::Sender<Map<String, Value>>,
    context: JobContext,
    rows: Arc<AtomicU64>,
}

impl ExportRowSink {
    /// 按单行背压发送一个已授权的数据行。
    pub async fn send(&self, row: Map<String, Value>) -> Result<()> {
        if self.context.is_cancelled() {
            return Err(task_error("queue_job_cancelled", "导出任务已取消"));
        }
        self.sender
            .send(row)
            .await
            .map_err(|_| task_error("data_exchange_export_stopped", "导出写入已停止"))?;
        let rows = self.rows.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if rows.is_multiple_of(256) {
            self.context
                .set_progress(&json!({"rows_written": rows}))
                .await?;
        }
        Ok(())
    }
}

/// 导出任务的持久化对象摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportTaskReport {
    /// 已写入的数据行数。
    pub rows_written: u64,
    /// 存储 disk 名称。
    pub disk: String,
    /// 应用私有的导出对象路径。
    pub path: String,
    /// 对象字节数。
    pub bytes_written: u64,
    /// 到期清理时间的 Unix 秒时间戳。
    pub expires_at_unix_secs: u64,
}

/// 由应用提供已授权行生产器，并将 CSV/XLSX 结果有界流式写入 Storage。
///
/// `produce_rows` 应只从已授权查询中读取记录，例如 `AuthorizedSelect::into_batches()`；
/// 此函数不接收 SQL，也不会替应用推断主体或业务授权。取消会丢弃临时文件，不发布半成品对象。
pub async fn export_storage_task<F, Fut>(
    context: &JobContext,
    disk_name: &str,
    output_path: StoragePath,
    format: ExchangeFormat,
    headers: Vec<String>,
    options: &ExportTaskOptions,
    produce_rows: F,
) -> Result<ExportTaskReport>
where
    F: FnOnce(ExportRowSink) -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    options.validate()?;
    validate_exchange_object_path(&output_path)?;
    let output_path =
        options.output_path_for_attempt(&output_path, context.id(), context.attempt())?;
    let storage = context.service::<Storage>().await?;
    let queue = context.service::<Queue>().await?;
    let disk = storage.disk(disk_name).await?;
    let temporary = tempfile::tempfile().map_err(Error::internal)?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Map<String, Value>>(1);
    let rows_written = Arc::new(AtomicU64::new(0));
    let sink = ExportRowSink {
        sender,
        context: context.clone(),
        rows: Arc::clone(&rows_written),
    };
    let writer_headers = headers.clone();
    let writer_options = options.writer.clone();
    let writer = tokio::task::spawn_blocking(move || {
        let rows = std::iter::from_fn(move || receiver.blocking_recv().map(Ok));
        let mut temporary = temporary;
        match format {
            ExchangeFormat::Csv => {
                write_csv(&mut temporary, &writer_headers, rows, &writer_options)?;
            }
            ExchangeFormat::Xlsx => write_xlsx(
                &mut temporary,
                "Export",
                &writer_headers,
                rows,
                &writer_options,
            )?,
        }
        let bytes = temporary.metadata().map_err(Error::internal)?.len();
        temporary
            .seek(SeekFrom::Start(0))
            .map_err(Error::internal)?;
        Ok::<_, Error>((temporary, bytes))
    });
    let producer_result = tokio::select! {
        () = context.cancelled() => Err(task_error("queue_job_cancelled", "导出任务已取消")),
        result = produce_rows(sink) => result,
    };
    if let Err(error) = producer_result {
        let _ = writer.await;
        return Err(error);
    }
    let (temporary, bytes_written) = writer
        .await
        .map_err(|_| task_error("data_exchange_writer_failed", "导出写入任务未能正常结束"))??;
    let rows = rows_written.load(Ordering::Relaxed);
    let content_type = match format {
        ExchangeFormat::Csv => "text/csv",
        ExchangeFormat::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    };
    let expires_at_unix_secs = unix_now().saturating_add(options.retention.as_secs());
    let upload_limit = options.writer.limits.max_output_bytes;
    let chunk_size = disk.max_chunk_bytes();
    publish_export_object(
        context,
        &queue,
        &disk,
        disk_name,
        &output_path,
        temporary,
        content_type,
        upload_limit,
        chunk_size,
        expires_at_unix_secs,
    )
    .await?;
    let report = ExportTaskReport {
        rows_written: rows,
        disk: disk_name.to_owned(),
        path: output_path.as_str().to_owned(),
        bytes_written,
        expires_at_unix_secs,
    };
    Ok(report)
}

#[allow(
    clippy::too_many_arguments,
    reason = "参数均来自已校验的任务上下文，合并会弱化 Storage 发布边界"
)]
async fn publish_export_object(
    context: &JobContext,
    queue: &Queue,
    disk: &StorageDisk,
    disk_name: &str,
    output_path: &StoragePath,
    temporary: std::fs::File,
    content_type: &str,
    upload_limit: u64,
    chunk_size: usize,
    expires_at_unix_secs: u64,
) -> Result<()> {
    let locks = SharedLocks::new((*context.service::<Db>().await?).clone());
    let context = context.clone();
    let queue = queue.clone();
    let disk = disk.clone();
    let disk_name = disk_name.to_owned();
    let output_path = output_path.clone();
    let content_type = content_type.to_owned();
    with_object_lock(locks, disk_name.clone(), output_path.clone(), async move {
        if context.is_cancelled() {
            return Err(task_error("queue_job_cancelled", "导出任务已取消"));
        }
        let body = file_stream(temporary, chunk_size);
        disk.put_stream(
            output_path.clone(),
            Some(content_type.clone()),
            body,
            UploadOptions {
                visibility: None,
                max_bytes: upload_limit,
                max_chunk_bytes: chunk_size,
                allowed_content_types: vec![content_type],
            },
            None,
        )
        .await?;
        if context.is_cancelled() {
            delete_storage_object_if_present(&disk, &output_path).await?;
            return Err(task_error("queue_job_cancelled", "导出任务已取消"));
        }
        write_retention_marker(&disk, &disk_name, &output_path, expires_at_unix_secs).await?;
        dispatch_report_cleanup(&queue, &disk_name, &output_path, expires_at_unix_secs).await?;
        Ok(())
    })
    .await
}

/// 导出临时文件和对象大小配置。
#[derive(Debug, Clone)]
pub struct ExportTaskOptions {
    /// CSV/XLSX writer 的行列、单元格及输出大小上限。
    pub writer: ExportOptions,
    /// 导出对象保留时长；到期后派发清理 job，最多 30 天。
    pub retention: Duration,
}

impl Default for ExportTaskOptions {
    fn default() -> Self {
        Self {
            writer: ExportOptions::default(),
            retention: Duration::from_hours(168),
        }
    }
}

impl ExportTaskOptions {
    /// 返回本次导出尝试的唯一 Storage 对象路径。
    pub fn output_path_for_attempt(
        &self,
        base_path: &StoragePath,
        job_id: &str,
        attempt: u64,
    ) -> Result<StoragePath> {
        attempt_object_path(base_path, "export", job_id, attempt, "")
    }

    fn validate(&self) -> Result<()> {
        if self.retention.is_zero() || self.retention > Duration::from_hours(720) {
            return Err(task_error(
                "data_exchange_task_options_invalid",
                "导出任务保留时长超出支持范围",
            ));
        }
        Ok(())
    }
}

const RETENTION_MARKER_FORMAT: u8 = 2;

/// 自动清理到期导出或导入错误报告的内置任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReportCleanupJob {
    /// 保留标记命名格式；旧持久任务缺失该字段时拒绝自动清理。
    #[serde(default)]
    pub marker_format: u8,
    /// 要清理的具名 disk。
    pub disk: String,
    /// 到期对象路径。
    pub path: String,
    /// 本次清理计划的 Unix 秒到期时间；对象被更新后旧计划不得删除新版本。
    pub expires_at_unix_secs: u64,
}

impl Job for ImportReportCleanupJob {
    const NAME: &'static str = "openrust.data_exchange.cleanup.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if self.marker_format != RETENTION_MARKER_FORMAT {
                return Err(task_error(
                    "data_exchange_cleanup_format_unsupported",
                    "旧清理任务需人工核查对象与保留标记，禁止自动删除",
                ));
            }
            let storage = context.service::<Storage>().await?;
            let queue = context.service::<Queue>().await?;
            let db = context.service::<Db>().await?;
            let path = StoragePath::new(self.path.clone())?;
            let disk = storage.disk(&self.disk).await?;
            let marker_path = retention_marker_path(&self.disk, &path)?;
            let locks = SharedLocks::new((*db).clone());
            let disk_name = self.disk.clone();
            let queue = queue.clone();
            let expires_at_unix_secs = self.expires_at_unix_secs;
            with_object_lock(locks, disk_name.clone(), path.clone(), async move {
                let current_expiry = match disk.open(&marker_path, None).await {
                    Ok(mut marker) => {
                        let mut bytes = Vec::new();
                        while let Some(chunk) = marker.body.next().await {
                            let chunk = chunk?;
                            if bytes.len().saturating_add(chunk.len()) > 128 {
                                return Err(task_error(
                                    "data_exchange_retention_marker_invalid",
                                    "对象保留标记超过大小上限",
                                ));
                            }
                            bytes.extend_from_slice(&chunk);
                        }
                        let current: RetentionMarker =
                            serde_json::from_slice(&bytes).map_err(|_| {
                                task_error(
                                    "data_exchange_retention_marker_invalid",
                                    "对象保留标记格式无效",
                                )
                            })?;
                        Some(current.expires_at_unix_secs)
                    }
                    Err(error) if error.code() == "storage_object_not_found" => None,
                    Err(error) => return Err(error),
                };
                let latest_expiry = current_expiry.unwrap_or(expires_at_unix_secs);
                if latest_expiry > unix_now() {
                    dispatch_report_cleanup(&queue, &disk_name, &path, latest_expiry).await?;
                    return Ok(());
                }
                delete_storage_object_if_present(&disk, &path).await?;
                delete_storage_object_if_present(&disk, &marker_path).await?;
                Ok(())
            })
            .await
        })
    }
}

async fn with_object_lock<F, T>(
    locks: SharedLocks,
    disk: String,
    path: StoragePath,
    operation: F,
) -> Result<T>
where
    F: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    tokio::spawn(async move {
        let lock_key = object_lock_key(&disk, &path);
        let lease = match locks.acquire(&lock_key, OBJECT_LOCK_TTL).await? {
            LockAcquire::Acquired(lease) => lease,
            LockAcquire::Busy { .. } => {
                return Err(task_error(
                    "data_exchange_object_busy",
                    "数据交换对象正在更新或清理",
                ));
            }
        };
        tokio::pin!(operation);
        let mut renewals = tokio::time::interval(OBJECT_LOCK_RENEW_INTERVAL);
        renewals.tick().await;
        let mut renewal_error = None;
        let result = loop {
            tokio::select! {
                result = &mut operation => break result,
                _ = renewals.tick() => match tokio::time::timeout(
                    OBJECT_LOCK_RENEW_TIMEOUT,
                    locks.renew(&lease, OBJECT_LOCK_TTL),
                ).await {
                    Ok(Ok(true)) => {},
                    Ok(Ok(false)) => renewal_error = Some(task_error(
                        "data_exchange_object_lock_lost",
                        "数据交换对象租约已被接管",
                    )),
                    Ok(Err(error)) => renewal_error = Some(error),
                    Err(_) => renewal_error = Some(task_error(
                        "data_exchange_object_lock_renew_timeout",
                        "数据交换对象租约续期超时",
                    )),
                }
            }
        };
        let released = locks.release(&lease).await;
        if let Some(error) = renewal_error {
            return Err(error);
        }
        match released {
            Ok(true) => result,
            Ok(false) => Err(task_error(
                "data_exchange_object_lock_lost",
                "数据交换对象租约已被接管",
            )),
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(Error::internal)?
}

fn object_lock_key(disk: &str, path: &StoragePath) -> String {
    let mut hasher = Sha256::new();
    hasher.update(disk.as_bytes());
    hasher.update([0]);
    hasher.update(path.as_str().as_bytes());
    format!("openrust.data-exchange.object.{}", hex(&hasher.finalize()))
}

async fn delete_storage_object_if_present(disk: &StorageDisk, path: &StoragePath) -> Result<()> {
    match disk.stat(path).await {
        Ok(_) => disk.delete(path).await,
        Err(error) if error.code() == "storage_object_not_found" => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RetentionMarker {
    expires_at_unix_secs: u64,
}

fn dispatch_report_cleanup(
    queue: &Queue,
    disk: &str,
    path: &StoragePath,
    expires_at_unix_secs: u64,
) -> BoxFuture<'static, Result<String>> {
    let queue = queue.clone();
    let payload = ImportReportCleanupJob {
        marker_format: RETENTION_MARKER_FORMAT,
        disk: disk.to_owned(),
        path: path.as_str().to_owned(),
        expires_at_unix_secs,
    };
    let delay = Duration::from_secs(expires_at_unix_secs.saturating_sub(unix_now()));
    let unique_key = cleanup_unique_key(disk, path, expires_at_unix_secs);
    Box::pin(async move {
        queue
            .dispatch_with_options(
                CLEANUP_QUEUE,
                ImportReportCleanupJob::NAME,
                &payload,
                DispatchOptions {
                    delay,
                    unique_key: Some(unique_key),
                    ..DispatchOptions::default()
                },
            )
            .await
    })
}

fn cleanup_unique_key(disk: &str, path: &StoragePath, expires_at_unix_secs: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(disk.as_bytes());
    hasher.update([0]);
    hasher.update(path.as_str().as_bytes());
    hasher.update(expires_at_unix_secs.to_be_bytes());
    format!("data-exchange-cleanup-v2-{}", hex(&hasher.finalize()[..16]))
}

fn retention_marker_path(disk: &str, path: &StoragePath) -> Result<StoragePath> {
    let mut hasher = Sha256::new();
    hasher.update(disk.as_bytes());
    hasher.update([0]);
    hasher.update(path.as_str().as_bytes());
    StoragePath::new(format!(
        ".data-exchange-retention/{}",
        hex(&hasher.finalize())
    ))
}

fn attempt_object_path(
    base_path: &StoragePath,
    purpose: &str,
    job_id: &str,
    attempt: u64,
    discriminator: &str,
) -> Result<StoragePath> {
    if job_id.is_empty() || attempt == 0 {
        return Err(task_error(
            "data_exchange_attempt_invalid",
            "任务 ID 与尝试次数无效",
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(job_id.as_bytes());
    hasher.update([0]);
    hasher.update(attempt.to_be_bytes());
    hasher.update([0]);
    hasher.update(discriminator.as_bytes());
    let suffix = format!(".{purpose}-{}-{attempt}", hex(&hasher.finalize()[..12]));
    StoragePath::new(format!("{}{suffix}", base_path.as_str()))
}

fn validate_exchange_object_path(path: &StoragePath) -> Result<()> {
    if [
        ".data-exchange-retention",
        ".openrust-data-exchange-retention",
    ]
    .iter()
    .any(|prefix| path.as_str() == *prefix || path.as_str().starts_with(&format!("{prefix}/")))
    {
        return Err(task_error(
            "data_exchange_path_reserved",
            "该 Storage 路径前缀由数据交换任务内部保留",
        ));
    }
    Ok(())
}

async fn write_retention_marker(
    disk: &StorageDisk,
    disk_name: &str,
    path: &StoragePath,
    expires_at_unix_secs: u64,
) -> Result<()> {
    let marker_path = retention_marker_path(disk_name, path)?;
    let bytes = serde_json::to_vec(&RetentionMarker {
        expires_at_unix_secs,
    })
    .map_err(Error::internal)?;
    disk.put_stream(
        marker_path,
        Some("application/json".to_owned()),
        Box::pin(stream::iter([Ok(Bytes::from(bytes))])),
        UploadOptions {
            visibility: None,
            max_bytes: 128,
            max_chunk_bytes: 128,
            allowed_content_types: vec!["application/json".to_owned()],
        },
        None,
    )
    .await?;
    Ok(())
}

fn idempotency_scopes(import_id: &str) -> Result<(String, String)> {
    let tenant = crate::tenant::TenantContext::current()
        .map_or_else(|| "default".to_owned(), |tenant| tenant.as_str().to_owned());
    let mut hasher = Sha256::new();
    hasher.update(tenant.as_bytes());
    let scope = format!(
        "openrust.data-exchange.{}.{}",
        hex(&hasher.finalize()[..12]),
        import_id
    );
    if scope.len() > 256 {
        return Err(task_error(
            "data_exchange_import_id_invalid",
            "导入 ID 超出长度上限",
        ));
    }
    Ok((scope.clone(), format!("{scope}.batch")))
}

fn validate_import_id(import_id: &str) -> Result<()> {
    if import_id.is_empty()
        || import_id.len() > 128
        || !import_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return Err(task_error(
            "data_exchange_import_id_invalid",
            "导入 ID 必须是 1 到 128 字节的安全 ASCII 标识符",
        ));
    }
    Ok(())
}

async fn store_report(
    _context: &JobContext,
    disk: &StorageDisk,
    path: &StoragePath,
    report: &ImportTaskReport,
) -> Result<()> {
    let bytes = serde_json::to_vec(report).map_err(Error::internal)?;
    if bytes.len() > 60 * 1024 {
        return Err(task_error(
            "data_exchange_report_too_large",
            "导入报告超过持久化大小上限",
        ));
    }
    let byte_length = bytes.len() as u64;
    let chunk_size = disk.max_chunk_bytes();
    let chunks = stream::unfold(Bytes::from(bytes), move |mut bytes| async move {
        if bytes.is_empty() {
            None
        } else {
            let length = bytes.len().min(chunk_size);
            Some((Ok(bytes.split_to(length)), bytes))
        }
    });
    disk.put_stream(
        path.clone(),
        Some("application/json".to_owned()),
        Box::pin(chunks),
        UploadOptions {
            visibility: None,
            max_bytes: byte_length,
            max_chunk_bytes: chunk_size,
            allowed_content_types: vec!["application/json".to_owned()],
        },
        None,
    )
    .await?;
    Ok(())
}

fn file_stream(file: std::fs::File, chunk_size: usize) -> StorageStream {
    use tokio::io::AsyncReadExt;

    let file = tokio::fs::File::from_std(file);
    Box::pin(stream::try_unfold(
        (file, chunk_size),
        |(mut file, chunk_size)| async move {
            let mut buffer = vec![0_u8; chunk_size];
            let read = file.read(&mut buffer).await.map_err(Error::internal)?;
            if read == 0 {
                return Ok(None);
            }
            buffer.truncate(read);
            Ok(Some((Bytes::from(buffer), (file, chunk_size))))
        },
    ))
}

fn bounded_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let boundary = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_bytes)
        .last()
        .unwrap_or(0);
    text[..boundary].to_owned()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn task_error(code: &'static str, message: &'static str) -> Error {
    Error::custom(code, message)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod naming_tests {
    use super::{attempt_object_path, retention_marker_path, validate_exchange_object_path};
    use crate::storage::StoragePath;

    #[test]
    fn exchange_paths_keep_attempts_distinct_and_reserve_both_marker_prefixes() -> crate::Result<()>
    {
        let base = StoragePath::new("reports/import.json")?;
        let first = attempt_object_path(&base, "import", "job-1", 1, "source-1")?;
        let retry = attempt_object_path(&base, "import", "job-1", 2, "source-1")?;
        assert!(first.as_str().starts_with("reports/import.json.import-"));
        assert_ne!(first, retry);
        let marker = retention_marker_path("private", &first)?;
        assert!(marker.as_str().starts_with(".data-exchange-retention/"));
        for value in [
            ".data-exchange-retention",
            ".data-exchange-retention/existing",
            ".openrust-data-exchange-retention",
            ".openrust-data-exchange-retention/existing",
        ] {
            assert!(validate_exchange_object_path(&StoragePath::new(value)?).is_err());
        }
        Ok(())
    }
}
