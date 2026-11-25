use openrust::database::{DatabaseError, Value};

use super::{CHECKS, ModerationStore, REVIEWS, database_clock_query, read_clock};
use crate::{
    ModerationError, ModerationResult, ProviderCancellation, ReviewState, TaskId, VideoReview,
    VideoSubmission,
};

pub(crate) enum Cancellation {
    Frozen,
    Provider { version: i64, task: TaskId },
}

impl ModerationStore {
    pub(crate) async fn cancel_active(
        &self,
        review: &VideoReview,
    ) -> ModerationResult<Option<Cancellation>> {
        if !matches!(review.state(), ReviewState::Pending | ReviewState::Running) {
            return Ok(None);
        }
        let version = review
            .version()
            .checked_add(1)
            .ok_or(ModerationError::ReviewStorage)?;
        let review = std::sync::Arc::new(review.clone());
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        self.connection
            .transaction(
                move |transaction| {
                    let (scope, clock, review) = (scope.clone(), clock.clone(), review.clone());
                    Box::pin(async move {
                        let now = read_clock(transaction, &clock).await?;
                        let (state, status, remote) = if review.deadline_at() <= now {
                            (ReviewState::Expired, Value::Null, Cancellation::Frozen)
                        } else {
                            let (status, remote) = provider_cancellation(&review, version);
                            (ReviewState::Cancelled, status.as_str().into(), remote)
                        };
                        let changed = transaction
                            .table(REVIEWS)
                            .r#where("id", "=", review.id())
                            .r#where("scope_key", "=", scope.as_str())
                            .r#where("version", "=", review.version())
                            .r#where("state", "=", review.state().as_str())
                            .update(
                                [
                                    ("state".into(), state.as_str().into()),
                                    ("version".into(), version.into()),
                                ]
                                .into_iter()
                                .collect(),
                            )
                            .await?;
                        if changed == 0 {
                            return Ok(None);
                        }
                        let saved = transaction
                            .table(CHECKS)
                            .r#where("id", "=", review.check_id())
                            .r#where("review_id", "=", review.id())
                            .update(
                                [
                                    ("provider_cancellation".into(), status),
                                    ("query_lease_until".into(), Value::Null),
                                    ("submit_lease_until".into(), Value::Null),
                                    ("dispatch_lease_until".into(), Value::Null),
                                ]
                                .into_iter()
                                .collect(),
                            )
                            .await?;
                        // 已过期的无租约检查可能没有额外字段可更新。
                        if saved > 1 || (saved == 0 && state == ReviewState::Cancelled) {
                            return Err(DatabaseError::InvalidQuery("无法记录内容审核取消"));
                        }
                        Ok(Some(remote))
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    pub(crate) async fn finish_cancellation(
        &self,
        review: &VideoReview,
        version: i64,
        status: ProviderCancellation,
    ) -> ModerationResult<()> {
        if status == ProviderCancellation::Unconfirmed {
            return Ok(());
        }
        let mut current = self
            .connection
            .table(REVIEWS)
            .where_column("moderation_reviews.id", "=", "moderation_checks.review_id")
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("version", "=", version)
            .r#where("state", "=", ReviewState::Cancelled.as_str());
        self.connection
            .table(CHECKS)
            .r#where("id", "=", review.check_id())
            .r#where("review_id", "=", review.id())
            .r#where(
                "provider_cancellation",
                "=",
                ProviderCancellation::Unconfirmed.as_str(),
            )
            .where_exists(&mut current)
            .map_err(|_| ModerationError::ReviewStorage)?
            .update(
                [("provider_cancellation".into(), status.as_str().into())]
                    .into_iter()
                    .collect(),
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        Ok(())
    }
}

fn provider_cancellation(
    review: &VideoReview,
    version: i64,
) -> (ProviderCancellation, Cancellation) {
    match review.submission() {
        VideoSubmission::Ready | VideoSubmission::Failed(_) | VideoSubmission::Expired => {
            (ProviderCancellation::NoTask, Cancellation::Frozen)
        }
        VideoSubmission::Submitting | VideoSubmission::Unknown => {
            (ProviderCancellation::TaskUnknown, Cancellation::Frozen)
        }
        VideoSubmission::Submitted(task) => (
            ProviderCancellation::Unconfirmed,
            Cancellation::Provider {
                version,
                task: task.clone(),
            },
        ),
    }
}
