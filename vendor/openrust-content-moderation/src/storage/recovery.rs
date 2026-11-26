use openrust::database::Value;

use crate::{ModerationError, ModerationResult};

use super::{CHECKS, ModerationStore, REVIEWS, database_clock, integer};

const DISPATCH_LEASE_SECONDS: i64 = 60;

pub(crate) struct RecoveryClaim {
    pub(crate) id: String,
    pub(crate) version: i64,
}

impl ModerationStore {
    /// 竞争派发短租约。队列失联后同一领域记录可再次被扫描。
    pub(crate) async fn claim_recovery(&self, limit: u32) -> ModerationResult<Vec<RecoveryClaim>> {
        let now = database_clock(&self.connection);
        let due = due_at(now);
        let dispatch_due =
            format!("(dispatch_lease_until is null or dispatch_lease_until <= {now})");
        let candidates = self
            .connection
            .table(REVIEWS)
            .use_write_pdo()
            .join(
                CHECKS,
                "moderation_reviews.id",
                "=",
                "moderation_checks.review_id",
            )
            .select([
                "moderation_reviews.id as review_id",
                "moderation_reviews.version",
            ])
            .select_raw(&format!("{now} as created_at"), Vec::new())
            .r#where("moderation_reviews.scope_key", "=", self.scope_key.as_str())
            .r#where("moderation_checks.kind", "=", "video")
            .where_raw(&due, Vec::new())
            .where_raw(&dispatch_due, Vec::new())
            .order_by("moderation_reviews.deadline_at", "asc")
            .order_by("moderation_reviews.id", "asc")
            .limit(u64::from(limit))
            .get()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        let mut claims = Vec::with_capacity(candidates.len());
        for row in candidates {
            let Some(Value::Text(id)) = row.get("review_id") else {
                return Err(ModerationError::ReviewStorage);
            };
            let version = integer(&row, "version").ok_or(ModerationError::ReviewStorage)?;
            let observed = integer(&row, "created_at").ok_or(ModerationError::ReviewStorage)?;
            let mut active = self
                .connection
                .table(REVIEWS)
                .where_column("moderation_reviews.id", "=", "moderation_checks.review_id")
                .r#where("moderation_reviews.scope_key", "=", self.scope_key.as_str())
                .r#where("moderation_reviews.version", "=", version)
                .where_raw(&due, Vec::new());
            let claimed = self
                .connection
                .table(CHECKS)
                .r#where("review_id", "=", id.as_str())
                .r#where("kind", "=", "video")
                .where_raw(&dispatch_due, Vec::new())
                .where_exists(&mut active)
                .map_err(|_| ModerationError::ReviewStorage)?
                .update(
                    [(
                        "dispatch_lease_until".into(),
                        (observed + DISPATCH_LEASE_SECONDS).into(),
                    )]
                    .into_iter()
                    .collect(),
                )
                .await
                .map_err(|_| ModerationError::ReviewStorage)?;
            if claimed == 1 {
                claims.push(RecoveryClaim {
                    id: id.clone(),
                    version,
                });
            }
        }
        Ok(claims)
    }
}

fn due_at(now: &str) -> String {
    format!(
        "state in ('Pending', 'Running') and (
            deadline_at <= {now}
            or (submission = 'Ready' and available_at <= {now})
            or (submission = 'Submitting' and submit_lease_until <= {now})
            or (submission = 'Submitted' and available_at <= {now}
                and (query_lease_until is null or query_lease_until <= {now}))
        )"
    )
}
