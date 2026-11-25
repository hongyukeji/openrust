use openrust::database::{DatabaseError, Transaction, Value};

use super::{ModerationStore, database_clock, digest};
use crate::{ModerationError, ModerationResult, TaskId};

const TOMBSTONES: &str = "moderation_video_tombstones";

impl ModerationStore {
    #[cfg(feature = "webhooks")]
    pub(crate) async fn has_video_tombstone(
        &self,
        driver: &str,
        account: &str,
        task: &TaskId,
    ) -> ModerationResult<bool> {
        let id = key(&self.scope_key, driver, account, task);
        let now = database_clock(&self.connection);
        self.connection
            .table(TOMBSTONES)
            .use_write_pdo()
            .r#where("id", "=", id.as_str())
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_raw(&format!("expires_at > {now}"), Vec::new())
            .exists()
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    /// 在本应用和租户内清除已超过保留时长的供应商任务关联。
    pub async fn prune_video_tombstones(&self, limit: u32) -> ModerationResult<u64> {
        if !(1..=1000).contains(&limit) {
            return Err(ModerationError::InvalidInput("video tombstone prune limit"));
        }
        let now = database_clock(&self.connection);
        let cutoff = format!("expires_at <= {now}");
        let rows = self
            .connection
            .table(TOMBSTONES)
            .use_write_pdo()
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_raw(&cutoff, Vec::new())
            .order_by("expires_at", "asc")
            .order_by("id", "asc")
            .select(["id"])
            .limit(u64::from(limit))
            .get()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        if rows.is_empty() {
            return Ok(0);
        }
        let ids = rows
            .iter()
            .map(|row| match row.get("id") {
                Some(Value::Text(id)) if id.len() == 64 => Ok(Value::from(id.as_str())),
                _ => Err(ModerationError::ReviewStorage),
            })
            .collect::<ModerationResult<Vec<_>>>()?;
        self.connection
            .table(TOMBSTONES)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_raw(&cutoff, Vec::new())
            .where_in("id", ids)
            .delete()
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }
}

pub(super) async fn retain(
    tx: &mut Transaction,
    scope: &str,
    driver: &str,
    account: &str,
    task: &TaskId,
    expires: i64,
) -> Result<(), DatabaseError> {
    let id = key(scope, driver, account, task);
    tx.table(TOMBSTONES)
        .upsert(
            &[[
                ("id".into(), id.as_str().into()),
                ("scope_key".into(), scope.into()),
                ("expires_at".into(), expires.into()),
            ]
            .into_iter()
            .collect()],
            &["id"],
            Some(&["id"]),
        )
        .await?;
    tx.table(TOMBSTONES)
        .r#where("id", "=", id.as_str())
        .r#where("scope_key", "=", scope)
        .r#where("expires_at", "<", expires)
        .update(
            [("expires_at".into(), expires.into())]
                .into_iter()
                .collect(),
        )
        .await?;
    Ok(())
}

fn key(scope: &str, driver: &str, account: &str, task: &TaskId) -> String {
    digest(&format!("{scope}\n{driver}\n{account}\n{}", task.as_str()))
}
