use openrust::database::{DatabaseError, DatabaseResult, Value, query::Record};

use super::{Settlement, TABLE, UsageAttempt, UsageStore, finish_usage};
use crate::{
    billing::{self, ChargePlan},
    budget,
    managed_keys::integer,
    output_quota,
};

const PENDING: &str = "background_pending";

impl UsageAttempt {
    pub(crate) async fn park_background(&self, resource: &str) -> DatabaseResult<()> {
        let changed = self
            .store
            .connection
            .table(TABLE)
            .r#where("id", "=", self.id.as_str())
            .where_not_null("dispatch_started_at")
            .where_null("finished_at")
            .update(Record::from([
                ("resource_id".into(), resource.into()),
                ("outcome".into(), PENDING.into()),
            ]))
            .await?;
        if changed != 1 {
            return Err(DatabaseError::InvalidQuery("后台响应出站记录不可用"));
        }
        Ok(())
    }
}

impl UsageStore {
    pub(crate) async fn background_pending(
        &self,
        attempt: &str,
        resource: &str,
    ) -> DatabaseResult<bool> {
        self.connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", attempt)
            .r#where("resource_id", "=", resource)
            .where_null("finished_at")
            .exists()
            .await
    }

    pub(crate) async fn finish_background(
        &self,
        attempt: &str,
        resource: &str,
        settlement: Settlement,
    ) -> DatabaseResult<()> {
        let attempt = attempt.to_owned();
        let resource = resource.to_owned();
        self.connection
            .transaction(
                move |transaction| {
                    let attempt = attempt.clone();
                    let resource = resource.clone();
                    let mut settlement = settlement.clone();
                    Box::pin(async move {
                        let row = transaction
                            .table(TABLE)
                            .r#where("id", "=", attempt.as_str())
                            .r#where("resource_id", "=", resource.as_str())
                            .r#where("upstream_endpoint", "=", "responses")
                            .first()
                            .await?
                            .ok_or(DatabaseError::InvalidScalar)?;
                        if integer(&row, "finished_at")?.is_some() {
                            return Ok(());
                        }
                        settlement.upstream_request_id = match row.get("upstream_request_id") {
                            Some(Value::Text(id)) => Some(id.clone()),
                            _ => None,
                        };
                        settlement.status = integer(&row, "status")?;
                        if finish_usage(transaction.table(TABLE), &attempt, &settlement).await? == 0
                        {
                            return Ok(());
                        }
                        let model = crate::managed_keys::text(&row, "upstream_model")?;
                        if let Some(plan) =
                            ChargePlan::recover(transaction, &attempt, model).await?
                        {
                            plan.settle(transaction, &attempt, &settlement).await?;
                            if let Some(Value::Text(id)) = row.get("budget_id") {
                                let reserved = integer(&row, "reserved_receivable_atoms")?
                                    .ok_or(DatabaseError::InvalidScalar)?;
                                budget::settle_reserved(
                                    transaction,
                                    id,
                                    reserved,
                                    plan.receivable(&settlement),
                                )
                                .await?;
                            }
                            if let Some(Value::Text(id)) = row.get("account_budget_id")
                                && !budget::settlements::applied(
                                    transaction,
                                    &attempt,
                                    billing::adjustments::Direction::Cost,
                                )
                                .await?
                            {
                                let reserved = integer(&row, "reserved_cost_atoms")?
                                    .ok_or(DatabaseError::InvalidScalar)?;
                                budget::settle_reserved(
                                    transaction,
                                    id,
                                    reserved,
                                    plan.cost(&settlement),
                                )
                                .await?;
                            }
                        }
                        if settlement.state == "final"
                            && let Some(Value::Text(id)) = row.get("output_quota_id")
                        {
                            let reserved = integer(&row, "reserved_output_tokens")?
                                .ok_or(DatabaseError::InvalidScalar)?;
                            output_quota::settle_reserved(
                                transaction,
                                id,
                                reserved,
                                settlement.output_tokens,
                            )
                            .await?;
                        }
                        Ok(())
                    })
                },
                3,
            )
            .await
    }
}
