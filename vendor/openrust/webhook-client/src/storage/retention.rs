use super::{DatabaseWebhookStore, attachments::AttachmentReference, scope_key, text};
use crate::{Error, Result};
use openrust::database::{Row, Value, query::Builder};

const MAX_PRUNABLE: u32 = 500;

impl DatabaseWebhookStore {
    fn retention_query<'a>(&self, builder: Builder<'a>, days: u32) -> Result<Builder<'a>> {
        if days > 36_500 {
            return Err(Error::InvalidConfig("delete_after_days"));
        }
        let seconds = Value::from(i64::from(days) * 86_400);
        let cutoff = format!("{} - ?", self.clock());
        Ok(builder
            .r#where("scope_key", "=", scope_key(&self.scope))
            .where_exact_text("scope", &self.scope)
            .where_null("processing_token")
            .where_raw(&format!("created_at < {cutoff}"), vec![seconds.clone()])
            .where_nested(|query| {
                query.where_raw(
                    &format!("processed_at < {cutoff} or processing_stopped_at < {cutoff}"),
                    vec![seconds.clone(), seconds],
                )
            }))
    }

    fn retention_candidates(&self, days: u32, limit: u32) -> Result<Builder<'static>> {
        if !(1..=MAX_PRUNABLE).contains(&limit) {
            return Err(Error::InvalidConfig("prune.limit"));
        }
        Ok(self
            .retention_query(self.connection.table("webhook_calls"), days)?
            .use_write_pdo()
            .order_by("created_at", "asc")
            .order_by("id", "asc")
            .limit(u64::from(limit)))
    }

    pub(crate) async fn prunable_ids(&self, days: u32, limit: u32) -> Result<Vec<String>> {
        self.retention_candidates(days, limit)?
            .select(["id"])
            .get()
            .await?
            .iter()
            .map(|row| text(row, "id").map(str::to_owned))
            .collect()
    }

    /// 清理已过保留期的终态记录，并在同一事务内登记需要回收的对象。
    pub async fn prune(&self, days: u32, limit: u32) -> Result<u64> {
        let rows = self
            .retention_candidates(days, limit)?
            .select(["id", "created_at", "attachment_reference"])
            .get()
            .await?;
        let mut inline = Vec::new();
        let mut stored = Vec::new();
        for row in &rows {
            let id = text(row, "id")?;
            match AttachmentReference::from_row(row)? {
                Some(reference) => stored.push((row, id, reference)),
                None => inline.push(Value::from(id)),
            }
        }
        let mut removed = 0;
        if !inline.is_empty() {
            removed = self
                .retention_query(self.connection.table("webhook_calls"), days)?
                .where_null("attachment_reference")
                .where_in("id", inline)
                .delete()
                .await?;
        }
        for (row, id, reference) in stored {
            removed += self.detach_attachment(days, row, id, &reference).await?;
        }
        Ok(removed)
    }

    async fn detach_attachment(
        &self,
        days: u32,
        row: &Row,
        id: &str,
        reference: &AttachmentReference,
    ) -> Result<u64> {
        let created_at = row.get("created_at").ok_or(Error::InvalidRecord)?.clone();
        let mut transaction = self.connection.begin_transaction().await?;
        let result = async {
            let removed = self
                .retention_query(transaction.table("webhook_calls"), days)?
                .r#where("id", "=", id)
                .where_not_null("attachment_reference")
                .delete()
                .await?;
            if removed == 1 {
                transaction
                    .table("webhook_attachment_staging")
                    .insert(&[[
                        ("id".into(), id.into()),
                        ("scope_key".into(), scope_key(&self.scope).into()),
                        ("disk".into(), reference.disk().into()),
                        ("created_at".into(), created_at),
                        ("detached".into(), true.into()),
                    ]
                    .into_iter()
                    .collect()])
                    .await?;
            }
            Ok(removed)
        }
        .await;
        match result {
            Ok(removed) => {
                transaction.commit().await?;
                Ok(removed)
            }
            Err(error) => {
                transaction.roll_back(None).await?;
                Err(error)
            }
        }
    }
}
