use crate::{ModerationError, ModerationResult, ReviewState, TaskId, VideoReview, VideoSubmission};

use super::{CHECKS, ModerationStore, REVIEWS, database_clock, decode, integer};

pub(crate) enum ProviderTask {
    Missing,
    Found(Box<VideoReview>),
    Ambiguous,
}

impl ModerationStore {
    pub(crate) async fn find_provider_task(
        &self,
        driver: &str,
        account: &str,
        task: &TaskId,
    ) -> ModerationResult<ProviderTask> {
        let rows = self
            .review_rows()
            .r#where("moderation_checks.remote_task_id", "=", task.as_str())
            .limit(17)
            .get()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        if rows.len() > 16 {
            return Ok(ProviderTask::Ambiguous);
        }
        let mut found = None;
        for row in rows {
            let review = decode(row)?;
            if review.profile().driver() == driver
                && review.profile().account() == account
                && matches!(review.submission(), VideoSubmission::Submitted(id) if id == task)
            {
                if found.is_some() {
                    return Ok(ProviderTask::Ambiguous);
                }
                found = Some(review);
            }
        }
        Ok(found.map_or(ProviderTask::Missing, |review| {
            ProviderTask::Found(Box::new(review))
        }))
    }

    pub(crate) async fn request_confirmation(
        &self,
        review: &VideoReview,
        receipt: &str,
    ) -> ModerationResult<bool> {
        let now = database_clock(&self.connection);
        let timestamp = self
            .connection
            .table(REVIEWS)
            .use_write_pdo()
            .select_raw(&format!("{now} as created_at"), Vec::new())
            .r#where("id", "=", review.id())
            .r#where("scope_key", "=", self.scope_key.as_str())
            .first()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?
            .and_then(|row| integer(&row, "created_at"))
            .ok_or(ModerationError::ReviewStorage)?;
        if review.deadline_at() <= timestamp {
            return Ok(true);
        }
        let mut current = self
            .connection
            .table(REVIEWS)
            .where_column("moderation_reviews.id", "=", "moderation_checks.review_id")
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("version", "=", review.version())
            .r#where("state", "=", ReviewState::Running.as_str())
            .where_raw(&format!("deadline_at > {now}"), Vec::new());
        let changed = self
            .connection
            .table(CHECKS)
            .r#where("id", "=", review.check_id())
            .r#where("review_id", "=", review.id())
            .r#where("submission", "=", "Submitted")
            .where_null("confirmation_callback_id")
            .where_exists(&mut current)
            .map_err(|_| ModerationError::ReviewStorage)?
            .update(
                [
                    ("confirmation_callback_id".into(), receipt.into()),
                    ("available_at".into(), timestamp.into()),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        Ok(changed == 1)
    }
}
