use openrust::database::{
    Row, Value,
    query::{Builder, Expression, UpdateValue},
};

use crate::{
    DeliveryStore, Error, Result,
    delivery_store::{ATTEMPTS, DELIVERIES, settle, text},
};

const MAX_PRUNE_LIMIT: u32 = 500;

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryRetention {
    Payloads,
    Attempts,
}

#[derive(Debug, serde::Serialize)]
pub struct DeliveryPruneReport {
    pub artifact: DeliveryRetention,
    pub pretend: bool,
    pub candidates: usize,
    pub pruned: u64,
}

impl DeliveryStore {
    /// 分批移除已过保留期的投递快照或尝试记录，并保留投递幂等身份。
    pub async fn prune(
        &self,
        artifact: DeliveryRetention,
        days: u32,
        limit: u32,
        pretend: bool,
    ) -> Result<DeliveryPruneReport> {
        if days > 36_500 || !(1..=MAX_PRUNE_LIMIT).contains(&limit) {
            return Err(Error::InvalidConfig("prune.days/limit"));
        }
        let seconds = i64::from(days) * 86_400;
        let candidates = self
            .retention_query(artifact, seconds)?
            .use_write_pdo()
            .order_by(
                match artifact {
                    DeliveryRetention::Payloads => "deadline",
                    DeliveryRetention::Attempts => "finished_at",
                },
                "asc",
            )
            .order_by("id", "asc")
            .limit(u64::from(limit))
            .select(
                match artifact {
                    DeliveryRetention::Payloads => ["id"].as_slice(),
                    DeliveryRetention::Attempts => ["id", "delivery_id"].as_slice(),
                }
                .iter()
                .copied(),
            )
            .get()
            .await?;
        let selected = candidates.len();
        let mut pruned = 0;
        if !pretend && !candidates.is_empty() {
            pruned = match artifact {
                DeliveryRetention::Payloads => {
                    self.retention_query(artifact, seconds)?
                        .where_in("id", row_ids(&candidates, "id")?)
                        .update_with(
                            [
                                (
                                    "payload_pruned_at".into(),
                                    UpdateValue::Operand(Expression::new(self.clock()).into()),
                                ),
                                (
                                    "snapshot".into(),
                                    UpdateValue::Operand(Value::from("").into()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                        .await?
                }
                DeliveryRetention::Attempts => self.prune_attempts(&candidates, seconds).await?,
            };
        }
        Ok(DeliveryPruneReport {
            artifact,
            pretend,
            candidates: selected,
            pruned,
        })
    }

    async fn prune_attempts(&self, candidates: &[Row], seconds: i64) -> Result<u64> {
        let parents = row_ids(candidates, "delivery_id")?;
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            if self.connection.get_driver_name() == "sqlite" {
                // SQLite 先取得写事务，防止在读快照上升级时与保留变更竞争。
                tx.table(DELIVERIES)
                    .r#where("scope_key", "=", self.scope_key.as_str())
                    .where_in("id", parents.clone())
                    .update_with(
                        [(
                            "retention_hold_at".into(),
                            UpdateValue::Operand(Expression::new("retention_hold_at").into()),
                        )]
                        .into_iter()
                        .collect(),
                    )
                    .await?;
            }
            let locked = self
                .terminal_deliveries(tx.table(DELIVERIES))
                .where_in("id", parents)
                .order_by("id", "asc")
                .select(["id"])
                .lock_for_update()
                .get()
                .await?;
            if locked.is_empty() {
                return Ok(0);
            }
            Ok(self
                .attempts_before(tx.table(ATTEMPTS), seconds)
                .where_in("id", row_ids(candidates, "id")?)
                .where_in("delivery_id", row_ids(&locked, "id")?)
                .delete()
                .await?)
        }
        .await;
        settle(tx, result).await
    }

    fn attempts_before<'a>(&self, query: Builder<'a>, seconds: i64) -> Builder<'a> {
        query.where_raw(
            &format!("finished_at < {} - ?", self.clock()),
            vec![seconds.into()],
        )
    }

    fn retention_query(
        &self,
        artifact: DeliveryRetention,
        seconds: i64,
    ) -> Result<Builder<'static>> {
        let cutoff = format!("{} - ?", self.clock());
        let scope = self.terminal_deliveries(self.connection.table(DELIVERIES));
        match artifact {
            DeliveryRetention::Payloads => Ok(scope
                .where_null("payload_pruned_at")
                .where_raw(&format!("deadline < {cutoff}"), vec![seconds.into()])),
            DeliveryRetention::Attempts => Ok(self
                .attempts_before(self.connection.table(ATTEMPTS), seconds)
                .where_in_sub("delivery_id", &mut scope.select(["id"]))?),
        }
    }

    fn terminal_deliveries<'a>(&self, query: Builder<'a>) -> Builder<'a> {
        query
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_null("lease_until")
            .where_null("retention_hold_at")
            .where_in(
                "state",
                ["failed", "expired", "succeeded", "cancelled"]
                    .map(Value::from)
                    .to_vec(),
            )
    }
}

fn row_ids(rows: &[Row], column: &str) -> Result<Vec<Value>> {
    rows.iter()
        .map(|row| text(row, column).map(|id| Value::from(id.to_owned())))
        .collect()
}
