use openrust::database::query::Builder;

use crate::{
    DeliveryStore, Error, IdempotencyKind, Result,
    delivery_store::{ATTEMPTS, REPLAYS, check_id, text},
    idempotency::KEYS,
    publication::MESSAGES,
};

#[derive(Debug, serde::Serialize)]
pub struct HistoryPruneReport {
    pub kind: IdempotencyKind,
    pub scanned: usize,
    pub candidates: usize,
    pub deleted: u64,
    pub pretend: bool,
    pub next_after: Option<String>,
}

impl DeliveryStore {
    /// 回收不再被尝试、重放或共享正文依赖的历史元数据。
    pub async fn prune_history(
        &self,
        kind: IdempotencyKind,
        days: u32,
        limit: u32,
        after: Option<&str>,
        pretend: bool,
    ) -> Result<HistoryPruneReport> {
        if days > 36_500 || !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("prune.days/limit"));
        }
        let seconds = i64::from(days) * 86_400;
        let mut query = self.history_eligible(kind, seconds)?.use_write_pdo();
        if let Some(after) = after {
            check_id(after)?;
            query = query.r#where("id", ">", after);
        }
        let rows = query
            .order_by("id", "asc")
            .limit(u64::from(limit) + 1)
            .select(["id"])
            .get()
            .await?;
        let page = &rows[..rows.len().min(limit as usize)];
        let next_after = if rows.len() > page.len() {
            page.last()
                .map(|row| text(row, "id").map(str::to_owned))
                .transpose()?
        } else {
            None
        };
        let mut report = HistoryPruneReport {
            kind,
            scanned: page.len(),
            candidates: 0,
            deleted: 0,
            pretend,
            next_after,
        };
        for row in page {
            let id = text(row, "id")?;
            check_id(id)?;
            // 旧发布使用原始回执保存引用。现有消息组固定，新发布不会采用已存在的投递 ID。
            if matches!(kind, IdempotencyKind::Delivery)
                && self
                    .connection
                    .table(MESSAGES)
                    .use_write_pdo()
                    .r#where("scope_key", "=", self.scope_key.as_str())
                    .where_null("body_pruned_at")
                    .where_json_contains("delivery_ids", serde_json::json!(id))
                    .exists()
                    .await?
            {
                continue;
            }
            report.candidates += 1;
            if !pretend {
                report.deleted += self
                    .history_eligible(kind, seconds)?
                    .r#where("id", "=", id)
                    .delete()
                    .await?;
            }
        }
        Ok(report)
    }

    fn history_eligible(&self, kind: IdempotencyKind, seconds: i64) -> Result<Builder<'static>> {
        let table = kind.table();
        let qualified = |field| format!("{table}.{field}");
        let record_id = qualified("id");
        let mut active_key = self
            .connection
            .table(KEYS)
            .r#where("kind", "=", kind.name())
            .where_column("scope_key", "=", &qualified("scope_key"))
            .where_column("record_id", "=", &record_id)
            .select(["id"]);
        let records = self
            .connection
            .table(table)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_null("retention_hold_at")
            .where_not_null(kind.pruned_at())
            .where_raw(
                &format!("idempotency_expired_at < {} - ?", self.clock()),
                vec![seconds.into()],
            )
            .where_not_exists(&mut active_key)?;
        if matches!(kind, IdempotencyKind::Publication) {
            return Ok(records);
        }

        let mut attempts = self
            .connection
            .table(ATTEMPTS)
            .where_column("delivery_id", "=", &record_id)
            .select(["id"]);
        let mut replays = self
            .connection
            .table(REPLAYS)
            .where_nested(|relation| {
                relation
                    .where_column("original_id", "=", &record_id)
                    .or_where_column("delivery_id", "=", &record_id)
            })
            .select(["delivery_id"]);
        Ok(records
            .where_not_exists(&mut attempts)?
            .where_not_exists(&mut replays)?)
    }
}
