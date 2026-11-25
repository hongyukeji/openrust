use openrust::database::{
    Value,
    query::{Builder, Expression, UpdateValue},
};

use crate::{
    DeliveryStore, Error, Result,
    delivery_store::{DELIVERIES, REPLAYS, check_id, text},
    publication::{MESSAGES, read_delivery_manifest},
};

#[derive(Debug, serde::Serialize)]
pub struct PublicationPruneReport {
    pub scanned: usize,
    pub candidates: usize,
    pub pruned: u64,
    pub pretend: bool,
    pub next_after: Option<String>,
}

impl DeliveryStore {
    /// 逐页回收已经没有投递快照引用的共享正文，保留发布回执。
    pub async fn prune_publication_bodies(
        &self,
        days: u32,
        limit: u32,
        after: Option<&str>,
        pretend: bool,
    ) -> Result<PublicationPruneReport> {
        if days > 36_500 || !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("prune.days/limit"));
        }
        let seconds = i64::from(days) * 86_400;
        let mut query = self.publication_retention_query(seconds).use_write_pdo();
        if let Some(after) = after {
            check_id(after)?;
            query = query.r#where("id", ">", after);
        }
        let rows = query
            .order_by("id", "asc")
            .limit(u64::from(limit) + 1)
            .select(["id", "delivery_ids"])
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
        let mut report = PublicationPruneReport {
            scanned: page.len(),
            candidates: 0,
            pruned: 0,
            pretend,
            next_after,
        };
        for row in page {
            let id = text(row, "id")?;
            check_id(id)?;
            let originals = read_delivery_manifest(text(row, "delivery_ids")?)?;
            if !self.publication_body_can_be_pruned(originals).await? {
                continue;
            }
            report.candidates += 1;
            if !pretend {
                report.pruned += self
                    .publication_retention_query(seconds)
                    .r#where("id", "=", id)
                    .update_with(
                        [
                            ("body".into(), UpdateValue::Operand(Value::from("").into())),
                            (
                                "body_pruned_at".into(),
                                UpdateValue::Operand(Expression::new(self.clock()).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )
                    .await?;
            }
        }
        Ok(report)
    }

    fn publication_retention_query(&self, seconds: i64) -> Builder<'static> {
        self.connection
            .table(MESSAGES)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_null("body_pruned_at")
            .where_null("retention_hold_at")
            .where_raw(
                &format!("created_at < {} - ?", self.clock()),
                vec![seconds.into()],
            )
    }

    async fn publication_body_can_be_pruned(&self, ids: Vec<String>) -> Result<bool> {
        if ids.is_empty() {
            return Ok(true);
        }
        let expected = ids.len() as u64;
        let ids = ids.into_iter().map(Value::from).collect::<Vec<_>>();
        let retired = self
            .connection
            .table(DELIVERIES)
            .use_write_pdo()
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_in("id", ids.clone())
            .where_not_null("payload_pruned_at")
            .count("*")
            .await?;
        if retired != expected {
            return Ok(false);
        }
        // 原投递一旦清理就不会再接受新重放。随后从写库检查已登记重放。
        let referenced = self
            .connection
            .table(REPLAYS)
            .use_write_pdo()
            .left_join(
                DELIVERIES,
                "webhook_delivery_replays.delivery_id",
                "=",
                "webhook_deliveries.id",
            )
            .where_in("webhook_delivery_replays.original_id", ids)
            .where_null("webhook_deliveries.payload_pruned_at")
            .exists()
            .await?;
        Ok(!referenced)
    }
}
