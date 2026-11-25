use crate::{Error, Result, VerifiedWebhook, models::WebhookCall};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{
    HeaderMap,
    header::{HeaderName, HeaderValue},
};
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
impl DatabaseWebhookStore {
    async fn prepare(
        &self,
        webhook: &VerifiedWebhook<'_>,
    ) -> Result<(WebhookCall, openrust::database::query::Record)> {
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
        let row = [
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
        Ok((call, row))
    }
}
impl WebhookStore for DatabaseWebhookStore {
    fn store_webhook<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>> {
        Box::pin(async move {
            let (call, row) = self.prepare(webhook).await?;
            self.connection
                .table("webhook_calls")
                .insert(&[row])
                .await?;
            Ok(call)
        })
    }

    fn store_and_dispatch_atomic<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
        dispatcher: &'a dyn crate::WebhookJobDispatcher,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            let (call, row) = self.prepare(webhook).await?;
            let mut transaction = self.connection.begin_transaction().await?;
            let queued = async {
                transaction.table("webhook_calls").insert(&[row]).await?;
                dispatcher.prepare(&call)?;
                dispatcher
                    .dispatch_in_transaction(&call, &mut transaction)
                    .await
            }
            .await;
            match queued {
                Ok(()) => transaction.commit().await.map_err(Into::into),
                Err(error) => {
                    transaction.roll_back(None).await?;
                    Err(error)
                }
            }
        })
    }
    fn find<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>> {
        Box::pin(async move { self.row(id, &["*"]).await?.map(decode).transpose() })
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
            crate::payload::attachments(&method, text(&row, "url")?, &raw_body(&row)?, &media_type)
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
            if changed == 0 && self.find(&call.id).await?.is_none() {
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

fn decode(row: Row) -> Result<WebhookCall> {
    let raw_body = raw_body(&row)?;
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
fn scope_key(scope: &str) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, scope.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
