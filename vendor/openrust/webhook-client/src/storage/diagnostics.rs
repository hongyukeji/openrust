use openrust::database::{Row, Value};

use super::{DatabaseWebhookStore, scope_key, text};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptState {
    Received,
    Processing,
    LeaseExpired,
    Deferred,
    Failed,
    Completed,
    Stopped,
}

#[derive(Debug, serde::Serialize)]
pub struct ReceiptDiagnostics {
    pub id: String,
    pub profile: String,
    pub revision: String,
    pub state: ReceiptState,
    pub received_at: i64,
    pub processed_at: Option<i64>,
    pub stopped_at: Option<i64>,
    pub next_processing_at: Option<i64>,
    pub processing_until: Option<i64>,
    pub generation: u64,
    pub replay_generation: u64,
    pub has_exception: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ProcessingAttempt {
    pub generation: u64,
    pub outcome: &'static str,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ProcessingAttemptsPage {
    pub data: Vec<ProcessingAttempt>,
    pub next_after: Option<u64>,
}

impl DatabaseWebhookStore {
    /// 读取当前授权范围内的接收状态。仅选择诊断所需列。
    pub async fn inspect_receipt(&self, id: &str) -> Result<Option<ReceiptDiagnostics>> {
        if id.len() != 36 {
            return Ok(None);
        }
        let row = self
            .receipt_query(id)
            .select([
                "scope",
                "name",
                "revision",
                "created_at",
                "processed_at",
                "processing_stopped_at",
                "processing_next_at",
                "processing_until",
                "processing_generation",
                "replay_generation",
            ])
            .select_raw(
                "case when exception is null then 0 else 1 end as has_exception",
                vec![],
            )
            .select_raw(
                &format!(
                    "case when processing_until > {} then 1 else 0 end as lease_active",
                    self.clock()
                ),
                vec![],
            )
            .first()
            .await?;
        row.filter(|row| self.matches_scope(row))
            .map(|row| receipt_diagnostics(id, &row))
            .transpose()
    }

    /// 按接收处理代次分页，每页 1 至 100 条。
    pub async fn inspect_processing_attempts(
        &self,
        id: &str,
        after: u64,
        limit: u32,
    ) -> Result<ProcessingAttemptsPage> {
        if !(1..=100).contains(&limit) {
            return Err(Error::InvalidConfig("attempts.limit"));
        }
        let Ok(after) = i64::try_from(after) else {
            return Ok(ProcessingAttemptsPage::default());
        };
        if id.len() != 36 {
            return Ok(ProcessingAttemptsPage::default());
        }
        let rows = self
            .connection
            .table("webhook_processing_attempts")
            .use_write_pdo()
            .join(
                "webhook_calls",
                "webhook_processing_attempts.call_id",
                "=",
                "webhook_calls.id",
            )
            .r#where("webhook_calls.scope_key", "=", scope_key(&self.scope))
            .r#where("webhook_processing_attempts.call_id", "=", id)
            .r#where("webhook_processing_attempts.generation", ">", after)
            .select([
                "webhook_calls.scope",
                "webhook_processing_attempts.generation",
                "outcome",
                "started_at",
                "finished_at",
            ])
            .order_by("webhook_processing_attempts.generation", "asc")
            .limit(u64::from(limit) + 1)
            .get()
            .await?;
        let mut data = rows
            .iter()
            .filter(|row| self.matches_scope(row))
            .map(processing_attempt)
            .collect::<Result<Vec<_>>>()?;
        let next_after = if data.len() > limit as usize {
            data.pop();
            data.last().map(|attempt| attempt.generation)
        } else {
            None
        };
        Ok(ProcessingAttemptsPage { data, next_after })
    }
}

fn receipt_diagnostics(id: &str, row: &Row) -> Result<ReceiptDiagnostics> {
    let processed_at = optional_number(row, "processed_at")?;
    let stopped_at = optional_number(row, "processing_stopped_at")?;
    let next_processing_at = optional_number(row, "processing_next_at")?;
    let processing_until = optional_number(row, "processing_until")?;
    let has_exception = required_number(row, "has_exception")? == 1;
    let state = if processed_at.is_some() {
        ReceiptState::Completed
    } else if stopped_at.is_some() {
        ReceiptState::Stopped
    } else if processing_until.is_some() {
        if required_number(row, "lease_active")? == 1 {
            ReceiptState::Processing
        } else {
            ReceiptState::LeaseExpired
        }
    } else if next_processing_at.is_some() {
        ReceiptState::Deferred
    } else if has_exception {
        ReceiptState::Failed
    } else {
        ReceiptState::Received
    };
    Ok(ReceiptDiagnostics {
        id: id.into(),
        profile: text(row, "name")?.into(),
        revision: text(row, "revision")?.into(),
        state,
        received_at: required_number(row, "created_at")?,
        processed_at,
        stopped_at,
        next_processing_at,
        processing_until,
        generation: generation(row, "processing_generation")?,
        replay_generation: generation(row, "replay_generation")?,
        has_exception,
    })
}

fn processing_attempt(row: &Row) -> Result<ProcessingAttempt> {
    let outcome = match text(row, "outcome")? {
        "running" => "running",
        "expired" => "expired",
        "released" => "released",
        "deferred" => "deferred",
        "failed" => "failed",
        "completed" => "completed",
        "stopped" => "stopped",
        _ => return Err(Error::InvalidRecord),
    };
    Ok(ProcessingAttempt {
        generation: generation(row, "generation")?,
        outcome,
        started_at: required_number(row, "started_at")?,
        finished_at: optional_number(row, "finished_at")?,
    })
}

fn generation(row: &Row, key: &str) -> Result<u64> {
    u64::try_from(required_number(row, key)?).map_err(|_| Error::InvalidRecord)
}

pub(super) fn required_number(row: &Row, key: &str) -> Result<i64> {
    optional_number(row, key)?.ok_or(Error::InvalidRecord)
}

fn optional_number(row: &Row, key: &str) -> Result<Option<i64>> {
    match row.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::Integer(value)) => Ok(Some(*value)),
        Some(Value::Unsigned(value)) => i64::try_from(*value)
            .map(Some)
            .map_err(|_| Error::InvalidRecord),
        _ => Err(Error::InvalidRecord),
    }
}
