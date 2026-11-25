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
}
impl WebhookStore for DatabaseWebhookStore {
    fn store_webhook<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>> {
        Box::pin(async move {
            let url = webhook.request().uri().to_string();
            if url.len() > 8192 || webhook.request().method().as_str().len() > 32 {
                return Err(Error::InvalidRecord);
            }
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
            self.connection
                .table("webhook_calls")
                .insert(&[[
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
                ]
                .into_iter()
                .collect()])
                .await?;
            Ok(call)
        })
    }
    fn find<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>> {
        Box::pin(async move {
            if id.len() != 36 {
                return Ok(None);
            }
            self.connection
                .table("webhook_calls")
                .r#where("scope_key", "=", scope_key(&self.scope))
                .r#where("id", "=", id)
                .first()
                .await?
                .map(decode)
                .transpose()
                .map(|call| call.filter(|call| call.scope == self.scope))
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
fn decode(row: Row) -> Result<WebhookCall> {
    fn text(row: &Row, key: &str) -> Result<String> {
        match row.get(key) {
            Some(Value::Text(value)) => Ok(value.clone()),
            _ => Err(Error::InvalidRecord),
        }
    }
    let wire = text(&row, "raw_body")?;
    if wire.len() > 16 * 1024 * 1024 * 4 / 3 + 4 {
        return Err(Error::InvalidRecord);
    }
    let raw_body = STANDARD.decode(wire).map_err(|_| Error::InvalidRecord)?;
    if raw_body.len() > 16 * 1024 * 1024 {
        return Err(Error::InvalidRecord);
    }
    let wire = text(&row, "headers")?;
    if wire.len() > 256 * 1024 {
        return Err(Error::InvalidRecord);
    }
    let pairs: Vec<(String, String)> =
        serde_json::from_str(&wire).map_err(|_| Error::InvalidRecord)?;
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
        id: text(&row, "id")?,
        scope: text(&row, "scope")?,
        name: text(&row, "name")?,
        revision: text(&row, "revision")?,
        url: text(&row, "url")?,
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
