use super::{
    DatabaseWebhookStore, attempts,
    lease::{assignment, settle_transaction},
    scope_key, text, update_time,
};
use crate::{
    Error, Result,
    replay::{ExhaustedReceipt, ReplayRequest},
};
use openrust::database::{
    Row, Value,
    query::{Builder, Expression},
};

const INTENTS: &str = "webhook_dispatch_intents";
const REPLAYS: &str = "webhook_replays";
const FIELDS: [&str; 5] = ["id", "scope", "name", "revision", "replay_generation"];

impl DatabaseWebhookStore {
    /// 只列出当前范围内可供申请重放的耗尽记录，不读取正文。
    pub async fn exhausted_receipts(&self, limit: u32) -> Result<Vec<ExhaustedReceipt>> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("exhausted.limit"));
        }
        self.exhausted(self.connection.table("webhook_calls"))?
            .use_write_pdo()
            .select(FIELDS)
            .order_by("id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?
            .iter()
            .map(decode)
            .collect()
    }

    pub(crate) async fn exhausted_receipt(&self, id: &str) -> Result<Option<ExhaustedReceipt>> {
        if id.len() != 36 {
            return Ok(None);
        }
        self.exhausted(self.connection.table("webhook_calls"))?
            .use_write_pdo()
            .select(FIELDS)
            .r#where("id", "=", id)
            .first()
            .await?
            .as_ref()
            .map(decode)
            .transpose()
    }

    pub(crate) async fn schedule_replay(&self, request: &ReplayRequest, ttl: u32) -> Result<()> {
        let id = request.receipt.id.as_str();
        let clock = self.clock();
        let scope = scope_key(&self.scope);
        let mut transaction = self.connection.begin_transaction().await?;
        let result = async {
            let times = transaction
                .select_one(
                    &format!("select {clock} as created_at, {clock} + ? as expires_at"),
                    &[i64::from(ttl).into()],
                )
                .await?
                .ok_or(Error::InvalidRecord)?;
            let expires_at = times
                .get("expires_at")
                .cloned()
                .ok_or(Error::InvalidRecord)?;
            let created_at = times
                .get("created_at")
                .cloned()
                .ok_or(Error::InvalidRecord)?;
            let claimed = self
                .exhausted(transaction.table("webhook_calls"))?
                .r#where("id", "=", id)
                .r#where(
                    "replay_generation",
                    "=",
                    i64::from(request.receipt.replay_generation),
                )
                .update_with(
                    [
                        assignment(
                            "replay_generation",
                            Value::from(i64::from(request.generation)),
                        ),
                        assignment("processing_deadline", expires_at.clone()),
                        assignment("processing_token", Value::Null),
                        assignment("processing_until", Value::Null),
                        assignment("updated_at", update_time(clock)),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if claimed != 1 {
                return Err(Error::ReplayUnavailable);
            }
            let opened = self
                .exhausted_intents(transaction.table(INTENTS))
                .r#where("call_id", "=", id)
                .r#where("scope_key", "=", scope.as_str())
                .update_with(
                    [
                        assignment("available_at", Expression::new(clock)),
                        assignment("enqueued_at", Value::Null),
                        assignment("claim_token", Value::Null),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if opened != 1 {
                return Err(Error::ReplayUnavailable);
            }
            attempts::expire(&mut transaction, id, clock).await?;
            transaction
                .table(REPLAYS)
                .insert(&[[
                    ("call_id".into(), id.into()),
                    ("generation".into(), i64::from(request.generation).into()),
                    ("actor".into(), request.actor.as_str().into()),
                    ("reason".into(), request.reason.as_str().into()),
                    ("created_at".into(), created_at),
                    ("expires_at".into(), expires_at),
                ]
                .into_iter()
                .collect()])
                .await?;
            Ok(())
        }
        .await;
        settle_transaction(transaction, result).await
    }

    pub(crate) async fn active_replay(
        &self,
        id: &str,
        generation: u32,
    ) -> Result<Option<(ReplayRequest, u64)>> {
        let Some(mut receipt) = self
            .row(id, &FIELDS)
            .await?
            .as_ref()
            .map(decode)
            .transpose()?
        else {
            return Ok(None);
        };
        if receipt.replay_generation != generation {
            return Ok(None);
        }
        receipt.replay_generation = generation - 1;
        let row = self
            .connection
            .table(REPLAYS)
            .use_write_pdo()
            .select(["actor", "reason", "expires_at"])
            .r#where("call_id", "=", id)
            .r#where("generation", "=", i64::from(generation))
            .where_raw(&format!("expires_at > {}", self.clock()), vec![])
            .first()
            .await?;
        row.map(|row| {
            let expires_at = match row.get("expires_at") {
                Some(Value::Integer(value)) => {
                    u64::try_from(*value).map_err(|_| Error::InvalidRecord)?
                }
                Some(Value::Unsigned(value)) => *value,
                _ => return Err(Error::InvalidRecord),
            };
            Ok((
                ReplayRequest {
                    receipt,
                    generation,
                    actor: text(&row, "actor")?.into(),
                    reason: text(&row, "reason")?.into(),
                },
                expires_at,
            ))
        })
        .transpose()
    }

    fn exhausted<'a>(&self, builder: Builder<'a>) -> Result<Builder<'a>> {
        let clock = self.clock();
        let scope = scope_key(&self.scope);
        let mut intent = self
            .exhausted_intents(self.connection.table(INTENTS))
            .select(["call_id"])
            .where_column("webhook_dispatch_intents.call_id", "=", "webhook_calls.id")
            .r#where("scope_key", "=", scope.as_str());
        Ok(builder
            .r#where("scope_key", "=", scope)
            .where_exact_text("scope", &self.scope)
            .where_null("processed_at")
            .where_null("processing_stopped_at")
            .where_null("processing_next_at")
            .where_nested(|query| {
                query
                    .where_null("processing_until")
                    .or_where_raw(&format!("processing_until <= {clock}"), vec![])
            })
            .where_nested(|query| {
                query
                    .where_null("processing_deadline")
                    .or_where_raw(&format!("processing_deadline > {clock}"), vec![])
            })
            .where_exists(&mut intent)?)
    }

    fn exhausted_intents<'a>(&self, builder: Builder<'a>) -> Builder<'a> {
        builder
            .where_not_null("processing_retry_after")
            .where_not_null("enqueued_at")
            .r#where("processing_requeues_left", "=", 0)
            .where_raw(&format!("available_at <= {}", self.clock()), vec![])
    }
}

fn decode(row: &Row) -> Result<ExhaustedReceipt> {
    Ok(ExhaustedReceipt {
        id: text(row, "id")?.into(),
        scope: text(row, "scope")?.into(),
        profile: text(row, "name")?.into(),
        revision: text(row, "revision")?.into(),
        replay_generation: generation(row)?,
    })
}

pub(super) fn generation(row: &Row) -> Result<u32> {
    let count = match row.get("replay_generation") {
        Some(Value::Integer(value)) => u32::try_from(*value),
        Some(Value::Unsigned(value)) => u32::try_from(*value),
        _ => return Err(Error::InvalidRecord),
    }
    .map_err(|_| Error::InvalidRecord)?;
    if count > 10 {
        return Err(Error::InvalidRecord);
    }
    Ok(count)
}
