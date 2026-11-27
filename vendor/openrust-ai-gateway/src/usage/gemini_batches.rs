use openrust::database::{DatabaseError, DatabaseResult, Row, Value, query::Record};
use openrust_ai::TextEndpoint;

use super::{Settlement, TABLE as USAGE_TABLE, UsageStore, finish_usage};
use crate::{
    billing::{self, ChargePlan},
    budget,
    managed_keys::{integer, now, text},
};

const TABLE: &str = "ai_gateway_gemini_batches";

#[derive(Clone)]
pub(crate) struct Batch {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) scope: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) upstream_model: String,
    pub(crate) upstream_name: Option<String>,
    pub(crate) endpoint: TextEndpoint,
    pub(crate) item_count: i32,
}

impl Batch {
    fn decode(row: &Row) -> DatabaseResult<Self> {
        let field = |column| text(row, column);
        let endpoint = match field("endpoint")?.as_str() {
            "batchGenerateContent" => TextEndpoint::GeminiBatchGenerateContent,
            "asyncBatchEmbedContent" => TextEndpoint::GeminiAsyncBatchEmbedContent,
            _ => return Err(DatabaseError::InvalidScalar),
        };
        Ok(Self {
            id: field("id")?,
            owner: field("owner")?,
            scope: field("scope")?,
            provider: field("provider")?,
            model: field("model")?,
            upstream_model: field("upstream_model")?,
            upstream_name: Some(field("upstream_name")?),
            endpoint,
            item_count: integer(row, "item_count")?
                .and_then(|count| i32::try_from(count).ok())
                .filter(|count| *count > 0)
                .ok_or(DatabaseError::InvalidScalar)?,
        })
    }
}

impl UsageStore {
    pub(crate) async fn reserve_gemini_batch(&self, batch: &Batch) -> DatabaseResult<()> {
        let record = Record::from([
            ("id".into(), batch.id.as_str().into()),
            ("owner".into(), batch.owner.as_str().into()),
            ("scope".into(), batch.scope.as_str().into()),
            ("provider".into(), batch.provider.as_str().into()),
            ("model".into(), batch.model.as_str().into()),
            (
                "upstream_model".into(),
                batch.upstream_model.as_str().into(),
            ),
            ("endpoint".into(), batch.endpoint.path().into()),
            ("item_count".into(), i64::from(batch.item_count).into()),
        ]);
        if !self.connection.table(TABLE).insert(&[record]).await? {
            return Err(DatabaseError::InvalidQuery("批任务登记失败"));
        }
        Ok(())
    }

    pub(crate) async fn attach_gemini_batch(
        &self,
        batch: &Batch,
        upstream: &str,
    ) -> DatabaseResult<()> {
        let changed = self
            .connection
            .table(TABLE)
            .r#where("id", "=", batch.id.as_str())
            .where_null("upstream_name")
            .update(Record::from([
                ("upstream_name".into(), upstream.into()),
                (
                    "upstream_identity".into(),
                    crate::session_affinity::key(&[&batch.scope, upstream]).into(),
                ),
            ]))
            .await?;
        if changed != 1 {
            return Err(DatabaseError::InvalidQuery("批任务上游状态已变更"));
        }
        Ok(())
    }

    pub(crate) async fn gemini_batch(
        &self,
        owner: &str,
        id: &str,
    ) -> DatabaseResult<Option<Batch>> {
        let row = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", id)
            .r#where("owner", "=", owner)
            .where_not_null("upstream_name")
            .where_null("deleted_at")
            .first()
            .await?;
        Ok(row
            .as_ref()
            .map(Batch::decode)
            .transpose()?
            .filter(|batch| batch.id == id && batch.owner == owner))
    }

    pub(crate) async fn gemini_batch_page(
        &self,
        owner: &str,
        cursor: Option<&str>,
        limit: u64,
    ) -> DatabaseResult<Vec<Batch>> {
        let mut query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("owner", "=", owner)
            .where_not_null("upstream_name")
            .where_null("deleted_at");
        if let Some(cursor) = cursor {
            query = query.r#where("id", "<", cursor);
        }
        query
            .order_by("id", "desc")
            .limit(limit)
            .get()
            .await?
            .iter()
            .map(Batch::decode)
            .filter(|batch| {
                batch.as_ref().is_err() || batch.as_ref().is_ok_and(|batch| batch.owner == owner)
            })
            .collect()
    }

    pub(crate) async fn settle_gemini_batch(
        &self,
        batch: &Batch,
        mut settlement: Settlement,
        deleted: bool,
    ) -> DatabaseResult<()> {
        let batch = batch.clone();
        settlement.upstream_endpoint = batch.endpoint.path().to_owned();
        settlement.input_only = batch.endpoint.is_embedding();
        self.connection
            .transaction(
                move |transaction| {
                    let batch = batch.clone();
                    let mut settlement = settlement.clone();
                    Box::pin(async move {
                        let row = transaction
                            .table(USAGE_TABLE)
                            .r#where("id", "=", batch.id.as_str())
                            .first()
                            .await?
                            .ok_or(DatabaseError::InvalidScalar)?;
                        // 请求 ID 与状态来自创建操作，查询不另记生成尝试。
                        settlement.upstream_request_id = match row.get("upstream_request_id") {
                            Some(Value::Text(id)) => Some(id.clone()),
                            _ => None,
                        };
                        settlement.status = integer(&row, "status")?;
                        let changed =
                            finish_usage(transaction.table(USAGE_TABLE), &batch.id, &settlement)
                                .await?;
                        if changed == 1
                            && let Some(plan) =
                                ChargePlan::recover(transaction, &batch.id, batch.upstream_model)
                                    .await?
                        {
                            plan.settle(transaction, &batch.id, &settlement).await?;
                            if let Some(Value::Text(id)) = row.get("budget_id") {
                                let amount = integer(&row, "reserved_receivable_atoms")?
                                    .ok_or(DatabaseError::InvalidScalar)?;
                                budget::settle_reserved(
                                    transaction,
                                    id,
                                    amount,
                                    plan.receivable(&settlement),
                                )
                                .await?;
                            }
                            if let Some(Value::Text(id)) = row.get("account_budget_id")
                                && !budget::settlements::applied(
                                    transaction,
                                    &batch.id,
                                    billing::adjustments::Direction::Cost,
                                )
                                .await?
                            {
                                let amount = integer(&row, "reserved_cost_atoms")?
                                    .ok_or(DatabaseError::InvalidScalar)?;
                                budget::settle_reserved(
                                    transaction,
                                    id,
                                    amount,
                                    plan.cost(&settlement),
                                )
                                .await?;
                            }
                        }
                        if deleted {
                            transaction
                                .table(TABLE)
                                .r#where("id", "=", batch.id.as_str())
                                .where_null("deleted_at")
                                .update(Record::from([("deleted_at".into(), now().into())]))
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
