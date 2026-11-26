use openrust::database::{DatabaseError, Transaction, Value};

use super::{ModerationStore, database_clock_query, digest, integer, read_clock};
use crate::{ModerationError, ModerationResult, review::valid_content_identifier};

const CONTROLS: &str = "moderation_revision_controls";
pub(super) const WITHDRAWN: &str = "内容审核修订已撤回";

impl ModerationStore {
    /// 应用完成授权后撤回该内容修订。同一作用域内的审核都使用同一撤回记录。
    pub async fn withdraw_video_revision(
        &self,
        content_id: &str,
        content_revision: &str,
    ) -> ModerationResult<i64> {
        if !valid_content_identifier(content_id, 256)
            || !valid_content_identifier(content_revision, 128)
        {
            return Err(ModerationError::InvalidInput("content ID or revision"));
        }
        let control = self.revision_control(content_id, content_revision);
        let clock = database_clock_query(&self.connection);
        self.connection
            .transaction(
                move |tx| {
                    let (control, clock) = (control.clone(), clock.clone());
                    Box::pin(async move {
                        if let Some(existing) = control.lock(tx).await? {
                            return Ok(existing);
                        }
                        let now = read_clock(tx, &clock).await?;
                        let updated = tx
                            .table(CONTROLS)
                            .r#where("id", "=", control.id.as_str())
                            .r#where("scope_key", "=", control.scope.as_str())
                            .where_null("withdrawn_at")
                            .update([("withdrawn_at".into(), now.into())].into_iter().collect())
                            .await?;
                        if updated != 1 {
                            return Err(DatabaseError::InvalidQuery("无法保存内容修订撤回"));
                        }
                        Ok(now)
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    pub(crate) async fn revision_is_withdrawn(
        &self,
        content_id: &str,
        content_revision: &str,
    ) -> ModerationResult<bool> {
        let control = self.revision_control(content_id, content_revision);
        self.connection
            .table(CONTROLS)
            .use_write_pdo()
            .r#where("id", "=", control.id.as_str())
            .r#where("scope_key", "=", control.scope.as_str())
            .where_not_null("withdrawn_at")
            .exists()
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    pub(super) fn revision_control(&self, content_id: &str, revision: &str) -> RevisionControl {
        RevisionControl {
            id: digest(&format!("{}\n{content_id}\n{revision}", self.scope_key)),
            scope: self.scope_key.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct RevisionControl {
    id: String,
    scope: String,
}

impl RevisionControl {
    pub(super) async fn lock(&self, tx: &mut Transaction) -> Result<Option<i64>, DatabaseError> {
        tx.table(CONTROLS)
            .upsert(
                &[[
                    ("id".into(), self.id.as_str().into()),
                    ("scope_key".into(), self.scope.as_str().into()),
                ]
                .into_iter()
                .collect()],
                &["id"],
                Some(&["id"]),
            )
            .await?;
        let invalid = || DatabaseError::InvalidQuery("无法读取内容修订控制状态");
        let row = tx
            .table(CONTROLS)
            .r#where("id", "=", self.id.as_str())
            .r#where("scope_key", "=", self.scope.as_str())
            .select(["withdrawn_at"])
            .lock_for_update()
            .first()
            .await?
            .ok_or_else(invalid)?;
        match row.get("withdrawn_at") {
            Some(Value::Null) => Ok(None),
            _ => integer(&row, "withdrawn_at").map(Some).ok_or_else(invalid),
        }
    }
}
