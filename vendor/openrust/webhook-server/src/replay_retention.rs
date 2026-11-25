use std::collections::BTreeSet;

use openrust::database::{Value, query::Builder};

use crate::{
    DeliveryStore, Error, Result,
    delivery_store::{DELIVERIES, REPLAYS, check_id, settle, text},
};

#[derive(Debug, serde::Serialize)]
pub struct ReplayAuditPruneReport {
    pub candidates: usize,
    pub deleted: u64,
    pub pretend: bool,
}

impl DeliveryStore {
    /// 清理已结束且两个投递均已过保留期的重放审计。
    pub async fn prune_replay_audit(
        &self,
        days: u32,
        limit: u32,
        pretend: bool,
    ) -> Result<ReplayAuditPruneReport> {
        if days > 36_500 || !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("prune.days/limit"));
        }
        let seconds = i64::from(days) * 86_400;
        let rows = self
            .replay_audit_eligible(self.connection.table(REPLAYS), seconds)?
            .use_write_pdo()
            .order_by("created_at", "asc")
            .order_by("delivery_id", "asc")
            .limit(u64::from(limit))
            .select(["original_id", "delivery_id"])
            .get()
            .await?;
        let candidates = rows.len();
        let mut deleted = 0;
        if !pretend && candidates > 0 {
            let mut parents = BTreeSet::new();
            let mut pairs = Vec::with_capacity(candidates);
            for row in &rows {
                let original = text(row, "original_id")?;
                let replay = text(row, "delivery_id")?;
                check_id(original)?;
                check_id(replay)?;
                parents.extend([original, replay]);
                pairs.push((original, replay));
            }
            let mut tx = self.connection.begin_transaction().await?;
            let result = async {
                let locked = self
                    .lock_prunable_deliveries(
                        &mut tx,
                        parents.into_iter().map(Value::from).collect(),
                    )
                    .await?;
                let locked = locked
                    .iter()
                    .map(|row| text(row, "id"))
                    .collect::<Result<BTreeSet<_>>>()?;
                let audit_ids = pairs
                    .into_iter()
                    .filter(|(original, replay)| {
                        locked.contains(original) && locked.contains(replay)
                    })
                    .map(|(_, replay)| Value::from(replay))
                    .collect::<Vec<_>>();
                if audit_ids.is_empty() {
                    return Ok(0);
                }
                Ok(self
                    .replay_audit_eligible(tx.table(REPLAYS), seconds)?
                    .where_in("delivery_id", audit_ids)
                    .delete()
                    .await?)
            }
            .await;
            deleted = settle(tx, result).await?;
        }
        Ok(ReplayAuditPruneReport {
            candidates,
            deleted,
            pretend,
        })
    }

    fn replay_audit_eligible<'a>(&self, query: Builder<'a>, seconds: i64) -> Result<Builder<'a>> {
        let parent = |column: &str| {
            self.terminal_deliveries(self.connection.table(DELIVERIES))
                .where_column("id", "=", column)
                .where_not_null("payload_pruned_at")
                .where_raw(
                    &format!("idempotency_expired_at < {} - ?", self.clock()),
                    vec![seconds.into()],
                )
                .select(["id"])
        };
        Ok(query
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_exists(&mut parent("webhook_delivery_replays.original_id"))?
            .where_exists(&mut parent("webhook_delivery_replays.delivery_id"))?)
    }
}
