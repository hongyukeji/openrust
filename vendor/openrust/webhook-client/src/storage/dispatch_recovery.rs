use super::{DatabaseWebhookStore, lease::assignment, scope_key, text};
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

    pub(crate) async fn due_dispatch_intents(&self, limit: u32) -> Result<Vec<Row>> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("recover.limit"));
        }
        Ok(self
            .due_dispatches()?
            .use_write_pdo()
            .select(["call_id", "route", "processing_retry_after"])
            .order_by("available_at", "asc")
            .order_by("call_id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?)
    }

    pub(crate) async fn claim_dispatch(&self, id: &str) -> Result<Option<String>> {
        let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let changed = self
            .due_dispatches()?
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
    ) -> Result<bool> {
        let builder =
            self.dispatch_intents()
                .r#where("call_id", "=", id)
                .r#where("claim_token", "=", token);
        let Some(after) = retry_after else {
            return Ok(builder.delete().await? == 1);
        };
        let clock = self.clock();
        let updated = builder
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
        Ok(updated == 1)
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

    pub(crate) async fn dispatch_metadata(
        &self,
        id: &str,
    ) -> Result<Option<(String, String, String)>> {
        let Some(row) = self.row(id, &["scope", "name", "revision"]).await? else {
            return Ok(None);
        };
        Ok(Some((
            self.scope.clone(),
            text(&row, "name")?.to_owned(),
            text(&row, "revision")?.to_owned(),
        )))
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

    fn due_dispatches(&self) -> Result<Builder<'static>> {
        let clock = self.clock();
        let mut ready = self
            .dispatch_receipt()
            .where_null("processed_at")
            .where_null("processing_stopped_at")
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
