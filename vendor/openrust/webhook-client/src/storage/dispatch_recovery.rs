use super::{
    DatabaseWebhookStore, attempts,
    diagnostics::required_number,
    lease::{assignment, settle_transaction, stopped_assignments},
    scope_key, text,
};
use crate::{Error, Result, webhook_config::ProcessingRecovery};
use openrust::{
    database::{
        Row, Transaction, Value,
        query::{Builder, Expression},
    },
    support::Str,
};

const TABLE: &str = "webhook_dispatch_intents";
const LEASE_SECONDS: u32 = 60;

/// 当前范围内可补派的有界计数及最早就绪时间。
#[derive(Debug, serde::Serialize)]
pub struct DispatchBacklog {
    pub ready: usize,
    pub oldest_ready_at: Option<i64>,
    pub has_more: bool,
}

pub(crate) struct DispatchMetadata {
    pub scope: String,
    pub profile: String,
    pub revision: String,
    pub tracks_deadline: bool,
    pub replay_generation: u32,
}

impl DatabaseWebhookStore {
    pub(super) async fn record_dispatch_intent(
        &self,
        transaction: &mut Transaction,
        id: &str,
        route: String,
        recovery: Option<&ProcessingRecovery>,
    ) -> Result<()> {
        let clock = self.clock();
        let timestamp = transaction
            .select_one(&format!("select {clock} as available_at"), &[])
            .await?
            .and_then(|row| row.get("available_at").cloned())
            .ok_or(Error::InvalidRecord)?;
        transaction
            .table(TABLE)
            .insert(&[[
                ("call_id".into(), id.into()),
                ("scope_key".into(), scope_key(&self.scope).into()),
                ("route".into(), route.into()),
                ("available_at".into(), timestamp),
                (
                    "processing_retry_after".into(),
                    recovery.map_or(Value::Null, |policy| i64::from(policy.after_seconds).into()),
                ),
                (
                    "processing_requeues_left".into(),
                    i64::from(recovery.map_or(0, |policy| policy.max_requeues)).into(),
                ),
            ]
            .into_iter()
            .collect()])
            .await?;
        Ok(())
    }

    pub(crate) async fn stage_deferred_dispatch(
        &self,
        transaction: &mut Transaction,
        id: &str,
        route: String,
    ) -> Result<()> {
        let scope = scope_key(&self.scope);
        let due = transaction
            .table("webhook_calls")
            .select(["processing_next_at"])
            .r#where("id", "=", id)
            .r#where("scope_key", "=", scope.as_str())
            .where_exact_text("scope", &self.scope)
            .first()
            .await?
            .and_then(|row| row.get("processing_next_at").cloned())
            .filter(|value| matches!(value, Value::Integer(_) | Value::Unsigned(_)))
            .ok_or(Error::InvalidRecord)?;
        transaction
            .table(TABLE)
            .upsert(
                &[[
                    ("call_id".into(), id.into()),
                    ("scope_key".into(), scope.into()),
                    ("route".into(), route.into()),
                    ("available_at".into(), due),
                    ("enqueued_at".into(), Value::Null),
                    ("claim_token".into(), Value::Null),
                ]
                .into_iter()
                .collect()],
                &["call_id"],
                Some(&["route", "available_at", "enqueued_at", "claim_token"]),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn expire_deferred_dispatches(&self, limit: u32) -> Result<()> {
        let mut expired = self.expired_deferred(self.dispatch_receipt());
        let ids = self
            .dispatch_intents()
            .where_exists(&mut expired)?
            .use_write_pdo()
            .select(["call_id"])
            .order_by("available_at", "asc")
            .order_by("call_id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?;

        let clock = self.clock();
        let scope = scope_key(&self.scope);
        for row in ids {
            let id = text(&row, "call_id")?;
            let mut transaction = self.connection.begin_transaction().await?;
            let result = async {
                let changed = self
                    .expired_deferred(transaction.table("webhook_calls"))
                    .r#where("id", "=", id)
                    .r#where("scope_key", "=", scope.as_str())
                    .where_exact_text("scope", &self.scope)
                    .update_with(stopped_assignments(clock))
                    .await?;
                if changed == 1 {
                    attempts::expire(&mut transaction, id, clock).await?;
                }
                Ok(())
            }
            .await;
            settle_transaction(transaction, result).await?;
        }
        Ok(())
    }

    fn expired_deferred<'a>(&self, builder: Builder<'a>) -> Builder<'a> {
        let clock = self.clock();
        builder
            .where_null("processed_at")
            .where_null("processing_stopped_at")
            .where_raw(&format!("processing_deadline <= {clock}"), vec![])
            .where_nested(|query| {
                query
                    .where_null("processing_until")
                    .or_where_raw(&format!("processing_until <= {clock}"), vec![])
            })
    }

    pub(crate) async fn due_dispatch_intents(&self, limit: u32) -> Result<Vec<Row>> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("recover.limit"));
        }
        Ok(self
            .ordered_due_dispatches()?
            .select(["call_id", "route", "processing_retry_after"])
            .limit(u64::from(limit))
            .get()
            .await?)
    }

    /// 预览补派积压。超过上限时只报告至少有该数量的记录。
    pub async fn inspect_dispatch_backlog(&self, limit: u32) -> Result<DispatchBacklog> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("recover.limit"));
        }
        let ready = self
            .ordered_due_dispatches()?
            .select(["available_at"])
            .limit(u64::from(limit) + 1)
            .get()
            .await?;
        let limit = limit as usize;
        Ok(DispatchBacklog {
            ready: ready.len().min(limit),
            oldest_ready_at: ready
                .first()
                .map(|row| required_number(row, "available_at"))
                .transpose()?,
            has_more: ready.len() > limit,
        })
    }

    fn ordered_due_dispatches(&self) -> Result<Builder<'static>> {
        Ok(self
            .due_dispatches(None)?
            .use_write_pdo()
            .order_by("available_at", "asc")
            .order_by("call_id", "asc"))
    }

    pub(crate) async fn claim_dispatch(&self, id: &str, generation: u32) -> Result<Option<String>> {
        let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let changed = self
            .due_dispatches(Some(generation))?
            .r#where("call_id", "=", id)
            .update_with(
                [
                    assignment("claim_token", token.clone()),
                    assignment("available_at", Expression::new(format!("{} + {LEASE_SECONDS}", self.clock()))),
                    assignment("processing_requeues_left", Expression::new(
                        "case when enqueued_at is null then processing_requeues_left else processing_requeues_left - 1 end",
                    )),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        Ok((changed == 1).then_some(token))
    }

    pub(crate) async fn finish_dispatch(
        &self,
        id: &str,
        token: &str,
        retry_after: Option<i64>,
        tracks_deadline: bool,
    ) -> Result<()> {
        let builder =
            self.dispatch_intents()
                .r#where("call_id", "=", id)
                .r#where("claim_token", "=", token);
        if retry_after.is_none() && !tracks_deadline {
            builder.delete().await?;
            return Ok(());
        }
        let after = retry_after.unwrap_or_default();
        let clock = self.clock();
        // Worker 可能已提交下一次延期，旧派发令牌不能覆盖新意图。
        builder
            .update_with(
                [
                    assignment("claim_token", Value::Null),
                    assignment("enqueued_at", Expression::new(clock)),
                    assignment(
                        "available_at",
                        Expression::new(format!("{clock} + ?")).with_bindings(vec![after.into()]),
                    ),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn prune_settled_dispatches(&self, limit: u32) -> Result<()> {
        let mut settled = self.dispatch_receipt().where_nested(|query| {
            query
                .where_not_null("processed_at")
                .or_where_not_null("processing_stopped_at")
        });
        let ids = self
            .dispatch_intents()
            .where_exists(&mut settled)?
            .use_write_pdo()
            .order_by("call_id", "asc")
            .limit(u64::from(limit))
            .pluck("call_id")
            .await?;
        if !ids.is_empty() {
            self.dispatch_intents()
                .where_in("call_id", ids)
                .where_exists(&mut settled)?
                .delete()
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn dispatch_metadata(&self, id: &str) -> Result<Option<DispatchMetadata>> {
        let Some(row) = self
            .row(
                id,
                &[
                    "scope",
                    "name",
                    "revision",
                    "processing_deadline",
                    "replay_generation",
                ],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(DispatchMetadata {
            scope: self.scope.clone(),
            profile: text(&row, "name")?.to_owned(),
            revision: text(&row, "revision")?.to_owned(),
            replay_generation: super::replay::generation(&row)?,
            tracks_deadline: matches!(
                row.get("processing_deadline"),
                Some(Value::Integer(_) | Value::Unsigned(_))
            ),
        }))
    }

    fn dispatch_intents(&self) -> Builder<'static> {
        self.connection
            .table(TABLE)
            .r#where("scope_key", "=", scope_key(&self.scope))
    }

    fn dispatch_receipt(&self) -> Builder<'static> {
        self.connection
            .table("webhook_calls")
            .select(["id"])
            .where_column("webhook_calls.id", "=", "webhook_dispatch_intents.call_id")
            .r#where("scope_key", "=", scope_key(&self.scope))
            .where_exact_text("scope", &self.scope)
    }

    fn due_dispatches(&self, generation: Option<u32>) -> Result<Builder<'static>> {
        let clock = self.clock();
        let mut ready = self
            .dispatch_receipt()
            .where_null("processed_at")
            .where_null("processing_stopped_at")
            .where_nested(|query| {
                query
                    .where_null("processing_deadline")
                    .or_where_raw(&format!("processing_deadline > {clock}"), vec![])
            })
            .where_nested(|query| {
                query
                    .where_null("processing_until")
                    .or_where_raw(&format!("processing_until <= {clock}"), vec![])
            })
            .where_nested(|query| {
                query
                    .where_null("processing_next_at")
                    .or_where_raw(&format!("processing_next_at <= {clock}"), vec![])
            });
        if let Some(generation) = generation {
            ready = ready.r#where("replay_generation", "=", i64::from(generation));
        }
        Ok(self
            .dispatch_intents()
            .where_raw(&format!("available_at <= {clock}"), vec![])
            .where_nested(|query| {
                query
                    .where_null("enqueued_at")
                    .or_where("processing_requeues_left", ">", 0)
            })
            .where_exists(&mut ready)?)
    }
}
