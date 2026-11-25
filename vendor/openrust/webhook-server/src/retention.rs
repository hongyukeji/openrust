use openrust::database::{
    Value,
    query::{Builder, Expression, UpdateValue},
};

use crate::{
    DeliveryStore, Error, Result,
    delivery_store::{ATTEMPTS, DELIVERIES, text},
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
            .select(["id"])
            .get()
            .await?
            .iter()
            .map(|row| text(row, "id").map(|id| Value::from(id.to_owned())))
            .collect::<Result<Vec<_>>>()?;
        let selected = candidates.len();
        let mut pruned = 0;
        if !pretend && !candidates.is_empty() {
            let query = self
                .retention_query(artifact, seconds)?
                .where_in("id", candidates);
            pruned = match artifact {
                DeliveryRetention::Payloads => {
                    query
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
                DeliveryRetention::Attempts => query.delete().await?,
            };
        }
        Ok(DeliveryPruneReport {
            artifact,
            pretend,
            candidates: selected,
            pruned,
        })
    }

    fn retention_query(
        &self,
        artifact: DeliveryRetention,
        seconds: i64,
    ) -> Result<Builder<'static>> {
        let cutoff = format!("{} - ?", self.clock());
        let scope = self.terminal_deliveries();
        match artifact {
            DeliveryRetention::Payloads => Ok(scope
                .where_null("payload_pruned_at")
                .where_raw(&format!("deadline < {cutoff}"), vec![seconds.into()])),
            DeliveryRetention::Attempts => Ok(self
                .connection
                .table(ATTEMPTS)
                .where_raw(&format!("finished_at < {cutoff}"), vec![seconds.into()])
                .where_in_sub("delivery_id", &mut scope.select(["id"]))?),
        }
    }

    fn terminal_deliveries(&self) -> Builder<'static> {
        self.connection
            .table(DELIVERIES)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_null("lease_until")
            .where_in(
                "state",
                ["failed", "expired", "succeeded", "cancelled"]
                    .map(Value::from)
                    .to_vec(),
            )
    }
}
