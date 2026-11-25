use super::{DatabaseWebhookStore, scope_key};
use crate::{Error, Result};
use openrust::database::{Value, query::Builder};

const MAX_PRUNABLE: u32 = 500;

impl DatabaseWebhookStore {
    fn retention_query(&self, days: u32) -> Result<Builder<'static>> {
        if days > 36_500 {
            return Err(Error::InvalidConfig("delete_after_days"));
        }
        let seconds = Value::from(i64::from(days) * 86_400);
        let cutoff = format!("{} - ?", self.clock());
        Ok(self
            .connection
            .table("webhook_calls")
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

    pub(crate) async fn prunable_ids(&self, days: u32, limit: u32) -> Result<Vec<String>> {
        if !(1..=MAX_PRUNABLE).contains(&limit) {
            return Err(Error::InvalidConfig("prune.limit"));
        }
        self.retention_query(days)?
            .use_write_pdo()
            .order_by("created_at", "asc")
            .order_by("id", "asc")
            .limit(u64::from(limit))
            .pluck("id")
            .await?
            .into_iter()
            .map(|id| match id {
                Value::Text(id) => Ok(id),
                _ => Err(Error::InvalidRecord),
            })
            .collect()
    }

    /// 每次只清理有终态且已过保留期的接收记录。
    pub async fn prune(&self, days: u32, limit: u32) -> Result<u64> {
        let ids = self.prunable_ids(days, limit).await?;
        if ids.is_empty() {
            return Ok(0);
        }
        Ok(self
            .retention_query(days)?
            .where_in("id", ids.into_iter().map(Value::from).collect())
            .delete()
            .await?)
    }
}
