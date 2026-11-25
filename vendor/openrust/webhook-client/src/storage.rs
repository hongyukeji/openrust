mod attachments;
mod attempts;
mod identity;
mod lease;
mod retention;
use crate::{Error, Result, VerifiedWebhook, models::WebhookCall};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{
    HeaderMap,
    header::{HeaderName, HeaderValue},
};
pub(crate) use lease::{DeferredWakeup, ProcessingClaim, ProcessingOutcome};
use openrust::{
    database::{Connection, Row, Value},
    http::HttpFuture,
    support::Str,
};
use serde_json::Value as Json;

/// 存储适配不执行签名验证、业务回调或任务派发。
pub trait WebhookStore: Send + Sync {
    fn store_webhook<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>>;
    fn store_and_dispatch_atomic<'a>(
        &'a self,
        _webhook: &'a VerifiedWebhook<'_>,
        _dispatcher: &'a dyn crate::WebhookJobDispatcher,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async { Err(Error::InvalidConfig("atomic_database_queue")) })
    }
    fn payload<'a>(&'a self, _id: &'a str) -> HttpFuture<'a, Result<Option<Json>>> {
        Box::pin(async { Err(Error::PayloadUnsupported) })
    }
    fn get_attachments<'a>(
        &'a self,
        _id: &'a str,
    ) -> HttpFuture<'a, Result<openrust::http::UploadedFileValue>> {
        Box::pin(async { Err(Error::PayloadUnsupported) })
    }
    fn prunable(&self, _days: u32, _limit: u32) -> HttpFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Err(Error::RetentionUnsupported) })
    }
    fn find<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>>;
    fn set_exception<'a>(
        &'a self,
        call: &'a WebhookCall,
        exception: Option<&'a Json>,
    ) -> HttpFuture<'a, Result<()>>;
}

/// scope 是所有节点共同配置的稳定应用／租户标识，不得使用节点名或随机工作目录。
#[derive(Clone)]
pub struct DatabaseWebhookStore {
    connection: Connection,
    scope: String,
}
impl DatabaseWebhookStore {
    pub fn new(connection: Connection, scope: impl Into<String>) -> Result<Self> {
        let scope = scope.into();
        if scope.is_empty() || scope.len() > 128 || scope.chars().any(char::is_control) {
            return Err(Error::InvalidConfig("scope"));
        }
        Ok(Self { connection, scope })
    }
    async fn row(&self, id: &str, columns: &[&str]) -> Result<Option<Row>> {
        if id.len() != 36 {
            return Ok(None);
        }
        let row = self
            .connection
            .table("webhook_calls")
            .use_write_pdo()
            .select(columns.iter().copied())
            .r#where("scope_key", "=", scope_key(&self.scope))
            .r#where("id", "=", id)
            .first()
            .await?;
        Ok(row.filter(
            |row| matches!(row.get("scope"), Some(Value::Text(scope)) if scope == &self.scope),
        ))
    }
}
struct PreparedReceipt {
    call: WebhookCall,
    row: openrust::database::query::Record,
    attachment: Option<attachments::AttachmentReference>,
}

impl DatabaseWebhookStore {
    async fn prepare(&self, webhook: &VerifiedWebhook<'_>) -> Result<PreparedReceipt> {
        let url = webhook.request().uri().to_string();
        if url.len() > 8192 || webhook.request().method().as_str().len() > 32 {
            return Err(Error::InvalidRecord);
        }
        let payload = crate::payload::ReceiptPayload::prepare(webhook).await?;
        let headers = WebhookCall::headers_to_store(webhook.config(), webhook.request())?;
        let encoded_headers: Vec<_> = headers
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), STANDARD.encode(value.as_bytes())))
            .collect();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .ok_or(Error::InvalidRecord)?;
        let id = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let call = WebhookCall {
            id,
            scope: self.scope.clone(),
            name: webhook.config().name().into(),
            revision: webhook.config().revision().into(),
            url,
            method: webhook.request().method().clone(),
            headers,
            raw_body: webhook.request().body().to_vec(),
            created_at,
            exception: None,
        };
        let mut row: openrust::database::query::Record = [
            ("id".into(), call.id.clone().into()),
            ("scope".into(), self.scope.clone().into()),
            ("scope_key".into(), scope_key(&self.scope).into()),
            ("name".into(), call.name.clone().into()),
            ("revision".into(), call.revision.clone().into()),
            ("url".into(), call.url.clone().into()),
            ("method".into(), call.method.as_str().into()),
            (
                "headers".into(),
                serde_json::to_string(&encoded_headers)
                    .map_err(|_| Error::InvalidRecord)?
                    .into(),
            ),
            ("raw_body".into(), STANDARD.encode(&call.raw_body).into()),
            ("created_at".into(), created_at.into()),
            ("exception".into(), Value::Null),
            (
                "payload".into(),
                serde_json::to_string(&payload.input)
                    .map_err(|_| Error::InvalidRecord)?
                    .into(),
            ),
            (
                "attachment_media_type".into(),
                payload
                    .attachment_media_type
                    .map_or(Value::Null, Value::Text),
            ),
        ]
        .into_iter()
        .collect();
        let attachment = if matches!(row.get("attachment_media_type"), Some(Value::Text(_)))
            && let Some(disk) = webhook.config().attachment_disk()?
        {
            let reference = self.stage_attachment(&call, disk).await?;
            row.insert(
                "attachment_reference".into(),
                serde_json::to_string(&reference)
                    .map_err(|_| Error::InvalidRecord)?
                    .into(),
            );
            row.insert("raw_body".into(), "".into());
            Some(reference)
        } else {
            None
        };
        Ok(PreparedReceipt {
            call,
            row,
            attachment,
        })
    }

    async fn rollback_receipt(
        &self,
        transaction: &mut openrust::database::Transaction,
        id: &str,
        attachment: Option<&attachments::AttachmentReference>,
    ) -> Result<()> {
        transaction.roll_back(None).await?;
        if let Some(attachment) = attachment {
            self.discard_attachment(id, attachment).await;
        }
        Ok(())
    }

    async fn read_body(&self, row: &Row) -> Result<Vec<u8>> {
        match self.disk_body(row).await? {
            Some(body) => Ok(body),
            None => raw_body(row),
        }
    }
}
impl WebhookStore for DatabaseWebhookStore {
    fn store_webhook<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>> {
        Box::pin(async move {
            let PreparedReceipt {
                call,
                row,
                attachment,
            } = self.prepare(webhook).await?;
            if let Some(attachment) = &attachment {
                let mut transaction = self.connection.begin_transaction().await?;
                let persisted = async {
                    transaction.table("webhook_calls").insert(&[row]).await?;
                    self.finish_attachment(&mut transaction, call.id()).await
                }
                .await;
                if let Err(error) = persisted {
                    self.rollback_receipt(&mut transaction, call.id(), Some(attachment))
                        .await?;
                    return Err(error);
                }
                transaction.commit().await?;
            } else {
                self.connection
                    .table("webhook_calls")
                    .insert(&[row])
                    .await?;
            }
            Ok(call)
        })
    }

    fn store_and_dispatch_atomic<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
        dispatcher: &'a dyn crate::WebhookJobDispatcher,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            let identity = identity::EventIdentity::from_verified(&self.scope, webhook);
            let clock = self.clock();
            if let Some(identity) = &identity
                && identity.already_received(&self.connection, clock).await?
            {
                return Ok(());
            }
            let PreparedReceipt {
                call,
                row,
                attachment,
            } = self.prepare(webhook).await?;
            let mut transaction = self.connection.begin_transaction().await?;
            if let Err(error) = transaction.table("webhook_calls").insert(&[row]).await {
                self.rollback_receipt(&mut transaction, call.id(), attachment.as_ref())
                    .await?;
                return Err(error.into());
            }
            if let Some(identity) = &identity {
                let persisted = async {
                    let record = identity.record(&mut transaction, call.id(), clock).await?;
                    transaction
                        .table("webhook_event_receipts")
                        .insert(&[record])
                        .await?;
                    Ok::<(), Error>(())
                }
                .await;
                if let Err(error) = persisted {
                    self.rollback_receipt(&mut transaction, call.id(), attachment.as_ref())
                        .await?;
                    if identity.already_received(&self.connection, clock).await? {
                        return Ok(());
                    }
                    return Err(error);
                }
            }
            let queued = async {
                if attachment.is_some() {
                    self.finish_attachment(&mut transaction, call.id()).await?;
                }
                dispatcher.prepare(&call)?;
                dispatcher
                    .dispatch_in_transaction(&call, &mut transaction)
                    .await
            }
            .await;
            match queued {
                Ok(()) => transaction.commit().await.map_err(Into::into),
                Err(error) => {
                    self.rollback_receipt(&mut transaction, call.id(), attachment.as_ref())
                        .await?;
                    Err(error)
                }
            }
        })
    }
    fn prunable(&self, days: u32, limit: u32) -> HttpFuture<'_, Result<Vec<String>>> {
        Box::pin(self.prunable_ids(days, limit))
    }

    fn find<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>> {
        Box::pin(async move {
            let Some(row) = self.row(id, &["*"]).await? else {
                return Ok(None);
            };
            let body = self.read_body(&row).await?;
            Ok(Some(decode(row, body)?))
        })
    }

    fn payload<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<Json>>> {
        Box::pin(async move {
            let row = self
                .row(id, &["scope", "payload"])
                .await?
                .ok_or(Error::RecordNotFound)?;
            match row.get("payload") {
                Some(Value::Null) => Ok(None),
                Some(Value::Text(value)) => Ok(Some(
                    serde_json::from_str(value).map_err(|_| Error::InvalidRecord)?,
                )),
                _ => Err(Error::InvalidRecord),
            }
        })
    }

    fn get_attachments<'a>(
        &'a self,
        id: &'a str,
    ) -> HttpFuture<'a, Result<openrust::http::UploadedFileValue>> {
        Box::pin(async move {
            let row = self
                .row(
                    id,
                    &[
                        "scope",
                        "id",
                        "attachment_reference",
                        "attachment_media_type",
                        "raw_body",
                        "method",
                        "url",
                    ],
                )
                .await?
                .ok_or(Error::RecordNotFound)?;
            let media_type = match row.get("attachment_media_type") {
                Some(Value::Null) => return Ok(openrust::http::UploadedFileValue::Null),
                Some(Value::Text(value)) if value.len() <= 64 * 1024 => value.clone(),
                _ => return Err(Error::InvalidRecord),
            };
            let method = http::Method::from_bytes(text(&row, "method")?.as_bytes())
                .map_err(|_| Error::InvalidRecord)?;
            crate::payload::attachments(
                &method,
                text(&row, "url")?,
                &self.read_body(&row).await?,
                &media_type,
            )
            .await
        })
    }

    fn set_exception<'a>(
        &'a self,
        call: &'a WebhookCall,
        exception: Option<&'a Json>,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            if call.scope != self.scope {
                return Err(Error::RecordNotFound);
            }
            let value = exception
                .map(|v| serde_json::to_string(v).map_err(|_| Error::InvalidRecord))
                .transpose()?;
            if value.as_ref().is_some_and(|v| v.len() > 8192) {
                return Err(Error::InvalidRecord);
            }
            let changed = self
                .connection
                .table("webhook_calls")
                .r#where("scope_key", "=", scope_key(&self.scope))
                .r#where("id", "=", call.id.clone())
                .update(
                    [(
                        "exception".into(),
                        value.map(Value::Text).unwrap_or(Value::Null),
                    )]
                    .into_iter()
                    .collect(),
                )
                .await?;
            // MySQL 的无变化更新可能报告零行；存在性仍按相同 scope 确认。
            if changed == 0 && self.row(&call.id, &["scope"]).await?.is_none() {
                return Err(Error::RecordNotFound);
            }
            Ok(())
        })
    }
}
fn text<'a>(row: &'a Row, key: &str) -> Result<&'a str> {
    match row.get(key) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(Error::InvalidRecord),
    }
}

fn raw_body(row: &Row) -> Result<Vec<u8>> {
    let wire = text(row, "raw_body")?;
    if wire.len() > 16 * 1024 * 1024 * 4 / 3 + 4 {
        return Err(Error::InvalidRecord);
    }
    let bytes = STANDARD.decode(wire).map_err(|_| Error::InvalidRecord)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Error::InvalidRecord);
    }
    Ok(bytes)
}

fn decode(row: Row, raw_body: Vec<u8>) -> Result<WebhookCall> {
    let wire = text(&row, "headers")?;
    if wire.len() > 256 * 1024 {
        return Err(Error::InvalidRecord);
    }
    let pairs: Vec<(String, String)> =
        serde_json::from_str(wire).map_err(|_| Error::InvalidRecord)?;
    if pairs.len() > 128 {
        return Err(Error::InvalidRecord);
    }
    let mut headers = HeaderMap::new();
    let mut size = 0;
    for (name, wire) in pairs {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::InvalidRecord)?;
        let value = STANDARD.decode(wire).map_err(|_| Error::InvalidRecord)?;
        size += name.as_str().len() + value.len();
        if size > 64 * 1024 {
            return Err(Error::InvalidRecord);
        }
        headers.append(
            name,
            HeaderValue::from_bytes(&value).map_err(|_| Error::InvalidRecord)?,
        );
    }
    let created_at = match row.get("created_at") {
        Some(Value::Integer(value)) => *value,
        Some(Value::Unsigned(value)) => i64::try_from(*value).map_err(|_| Error::InvalidRecord)?,
        _ => return Err(Error::InvalidRecord),
    };
    let exception = match row.get("exception") {
        None | Some(Value::Null) => None,
        Some(Value::Text(value)) if value.len() <= 8192 => {
            Some(serde_json::from_str(value).map_err(|_| Error::InvalidRecord)?)
        }
        _ => return Err(Error::InvalidRecord),
    };
    Ok(WebhookCall {
        id: text(&row, "id")?.to_owned(),
        scope: text(&row, "scope")?.to_owned(),
        name: text(&row, "name")?.to_owned(),
        revision: text(&row, "revision")?.to_owned(),
        url: text(&row, "url")?.to_owned(),
        method: http::Method::from_bytes(text(&row, "method")?.as_bytes())
            .map_err(|_| Error::InvalidRecord)?,
        headers,
        raw_body,
        created_at,
        exception,
    })
}

// 使用固定摘要查询，避免数据库默认不区分大小写的排序规则混同应用／租户范围。
pub(crate) fn scope_key(scope: &str) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, scope.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn database_clock(connection: &Connection) -> &'static str {
    // 与原生数据库队列使用相同的数据库时钟。
    match connection.get_driver_name() {
        "sqlite" => "unixepoch()",
        "pgsql" => "floor(extract(epoch from statement_timestamp()))::bigint",
        "sqlsrv" => "datediff_big(second, '19700101', sysutcdatetime())",
        _ => "unix_timestamp()",
    }
}
