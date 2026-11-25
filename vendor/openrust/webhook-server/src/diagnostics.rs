use openrust::database::{Row, Value};

use crate::{
    DeliveryStore, Error, EventType, Result,
    delivery_store::{ATTEMPTS, DELIVERIES, check_id, number, text},
    publication::{MESSAGES, read_delivery_manifest},
};

#[derive(Debug, serde::Serialize)]
pub struct DeliveryDiagnostics {
    pub id: String,
    pub state: &'static str,
    pub attempts: u32,
    pub maximum_tries: u32,
    pub available_at: i64,
    pub deadline: i64,
    pub lease_until: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_pruned_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_expired_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_hold_at: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct MessageDiagnostics {
    pub id: String,
    pub event_type: String,
    pub schema_version: u32,
    pub created_at: i64,
    pub delivery_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_pruned_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_expired_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_hold_at: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct DeliveryAttemptDiagnostics {
    pub number: u32,
    pub outcome: &'static str,
    pub http_status: Option<u16>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct DeliveryAttemptsPage {
    pub data: Vec<DeliveryAttemptDiagnostics>,
    pub next_after: Option<u32>,
}

impl DeliveryStore {
    /// 读取同一范围的脱敏投递状态。调用应用负责授权该范围。
    pub async fn inspect_delivery(&self, id: &str) -> Result<Option<DeliveryDiagnostics>> {
        check_id(id)?;
        self.connection
            .table(DELIVERIES)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("id", "=", id)
            .select([
                "state",
                "attempts",
                "maximum_tries",
                "available_at",
                "deadline",
                "lease_until",
                "payload_pruned_at",
                "idempotency_expired_at",
                "retention_hold_at",
            ])
            .first()
            .await?
            .map(|row| {
                let state = match text(&row, "state")? {
                    "pending" => "pending",
                    "running" => "running",
                    "succeeded" => "succeeded",
                    "failed" => "failed",
                    "expired" => "expired",
                    "cancelled" => "cancelled",
                    _ => return Err(Error::InvalidRecord),
                };
                Ok(DeliveryDiagnostics {
                    id: id.to_owned(),
                    state,
                    attempts: unsigned(&row, "attempts")?,
                    maximum_tries: unsigned(&row, "maximum_tries")?,
                    available_at: number(&row, "available_at")?,
                    deadline: number(&row, "deadline")?,
                    lease_until: optional_number(&row, "lease_until")?,
                    payload_pruned_at: optional_number(&row, "payload_pruned_at")?,
                    idempotency_expired_at: optional_number(&row, "idempotency_expired_at")?,
                    retention_hold_at: optional_number(&row, "retention_hold_at")?,
                })
            })
            .transpose()
    }

    /// 通过发布回执的内部 ID 读取事件元数据与首次投递集合。
    pub async fn inspect_message(&self, id: &str) -> Result<Option<MessageDiagnostics>> {
        check_id(id)?;
        self.connection
            .table(MESSAGES)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("id", "=", id)
            .select([
                "topic",
                "schema_version",
                "created_at",
                "delivery_ids",
                "body_pruned_at",
                "idempotency_expired_at",
                "retention_hold_at",
            ])
            .first()
            .await?
            .map(|row| {
                let event_type = text(&row, "topic")?;
                let schema_version = unsigned(&row, "schema_version")?;
                EventType::new(event_type, schema_version).map_err(|_| Error::InvalidRecord)?;
                let delivery_ids = read_delivery_manifest(text(&row, "delivery_ids")?)?;
                Ok(MessageDiagnostics {
                    id: id.to_owned(),
                    event_type: event_type.to_owned(),
                    schema_version,
                    created_at: number(&row, "created_at")?,
                    delivery_ids,
                    body_pruned_at: optional_number(&row, "body_pruned_at")?,
                    idempotency_expired_at: optional_number(&row, "idempotency_expired_at")?,
                    retention_hold_at: optional_number(&row, "retention_hold_at")?,
                })
            })
            .transpose()
    }

    /// 按尝试序号分页，每页最多 100 条。未知或其他范围的投递返回空页。
    pub async fn inspect_attempts(
        &self,
        id: &str,
        after: u32,
        limit: u32,
    ) -> Result<DeliveryAttemptsPage> {
        check_id(id)?;
        if !(1..=100).contains(&limit) {
            return Err(Error::InvalidConfig("attempts.limit"));
        }
        if after >= i32::MAX as u32 {
            return Ok(DeliveryAttemptsPage {
                data: Vec::new(),
                next_after: None,
            });
        }
        let mut data = self
            .connection
            .table(ATTEMPTS)
            .join(
                DELIVERIES,
                "webhook_delivery_attempts.delivery_id",
                "=",
                "webhook_deliveries.id",
            )
            .r#where("webhook_deliveries.scope_key", "=", self.scope_key.as_str())
            .r#where("webhook_delivery_attempts.delivery_id", "=", id)
            .r#where("webhook_delivery_attempts.number", ">", i64::from(after))
            .select([
                "webhook_delivery_attempts.number",
                "webhook_delivery_attempts.outcome",
                "webhook_delivery_attempts.http_status",
                "webhook_delivery_attempts.started_at",
                "webhook_delivery_attempts.finished_at",
            ])
            .order_by("webhook_delivery_attempts.number", "asc")
            .limit(u64::from(limit) + 1)
            .get()
            .await?
            .iter()
            .map(attempt)
            .collect::<Result<Vec<_>>>()?;
        let next_after = if data.len() > limit as usize {
            data.pop();
            data.last().map(|attempt| attempt.number)
        } else {
            None
        };
        Ok(DeliveryAttemptsPage { data, next_after })
    }
}

fn attempt(row: &Row) -> Result<DeliveryAttemptDiagnostics> {
    let outcome = match text(row, "outcome")? {
        "running" => "running",
        "unknown" => "unknown",
        "unresolved" => "unresolved",
        "rejected" => "rejected",
        "stopped" => "stopped",
        "cancelled" => "cancelled",
        "succeeded" => "succeeded",
        "http_failure" => "http_failure",
        _ => return Err(Error::InvalidRecord),
    };
    let http_status = optional_number(row, "http_status")?
        .map(|status| {
            if (100..=999).contains(&status) {
                Ok(status as u16)
            } else {
                Err(Error::InvalidRecord)
            }
        })
        .transpose()?;
    Ok(DeliveryAttemptDiagnostics {
        number: unsigned(row, "number")?,
        outcome,
        http_status,
        started_at: number(row, "started_at")?,
        finished_at: optional_number(row, "finished_at")?,
    })
}

fn optional_number(row: &Row, key: &str) -> Result<Option<i64>> {
    if matches!(row.get(key), Some(Value::Null)) {
        Ok(None)
    } else {
        number(row, key).map(Some)
    }
}

fn unsigned(row: &Row, key: &str) -> Result<u32> {
    u32::try_from(number(row, key)?).map_err(|_| Error::InvalidRecord)
}
