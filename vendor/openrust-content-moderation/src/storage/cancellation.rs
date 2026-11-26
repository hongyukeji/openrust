use openrust::database::{DatabaseError, Transaction, Value};
use std::sync::Arc;

use super::{CHECKS, ModerationStore, REVIEWS, database_clock_query, read_clock, remote_leases};
use crate::{
    ModerationError, ModerationResult, ProviderCancellation, ReviewState, TaskId, VideoReview,
    VideoReviewGroup, VideoSubmission,
};

const GROUP_CHANGED: &str = "复合审核检查版本已变更";

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
        let review = Arc::new(review.clone());
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        self.connection
            .transaction(
                move |transaction| {
                    let (scope, clock, review) = (scope.clone(), clock.clone(), review.clone());
                    Box::pin(async move {
                        let now = read_clock(transaction, &clock).await?;
                        freeze_check(transaction, &scope, &review, now).await
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    pub(crate) async fn cancel_group_active(
        &self,
        group: &VideoReviewGroup,
    ) -> ModerationResult<Option<Vec<(VideoReview, Cancellation)>>> {
        let active = group
            .checks()
            .iter()
            .filter(|check| matches!(check.state(), ReviewState::Pending | ReviewState::Running))
            .cloned()
            .collect::<Vec<_>>();
        let active = Arc::new(active);
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        let result = self
            .connection
            .transaction(
                move |transaction| {
                    let (scope, clock, active) = (scope.clone(), clock.clone(), active.clone());
                    Box::pin(async move {
                        let now = read_clock(transaction, &clock).await?;
                        let mut frozen = Vec::with_capacity(active.len());
                        for check in active.iter() {
                            let cancellation = freeze_check(transaction, &scope, check, now)
                                .await?
                                .ok_or(DatabaseError::InvalidQuery(GROUP_CHANGED))?;
                            frozen.push((check.clone(), cancellation));
                        }
                        Ok(frozen)
                    })
                },
                3,
            )
            .await;
        match result {
            Ok(cancellations) => Ok(Some(cancellations)),
            Err(DatabaseError::InvalidQuery(GROUP_CHANGED)) => Ok(None),
            Err(_) => Err(ModerationError::ReviewStorage),
        }
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
        let scope = self.scope_key.clone();
        let (id, check) = (review.id().to_owned(), review.check_id().to_owned());
        self.connection
            .transaction(
                move |tx| {
                    let (scope, id, check) = (scope.clone(), id.clone(), check.clone());
                    Box::pin(async move {
                        let cancelled = tx
                            .table(REVIEWS)
                            .r#where("id", "=", id.as_str())
                            .r#where("scope_key", "=", scope.as_str())
                            .r#where("version", "=", version)
                            .r#where("state", "=", ReviewState::Cancelled.as_str())
                            .select(["id"])
                            .lock_for_update()
                            .first()
                            .await?;
                        if cancelled.is_none() {
                            return Ok(());
                        }
                        let saved = tx
                            .table(CHECKS)
                            .r#where("id", "=", check.as_str())
                            .r#where("review_id", "=", id.as_str())
                            .r#where(
                                "provider_cancellation",
                                "=",
                                ProviderCancellation::Unconfirmed.as_str(),
                            )
                            .update(
                                [("provider_cancellation".into(), status.as_str().into())]
                                    .into_iter()
                                    .collect(),
                            )
                            .await?;
                        if saved == 1 && status == ProviderCancellation::Acknowledged {
                            remote_leases::release(tx, &check).await?;
                        }
                        Ok(())
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }
}

async fn freeze_check(
    transaction: &mut Transaction,
    scope: &str,
    review: &VideoReview,
    now: i64,
) -> Result<Option<Cancellation>, DatabaseError> {
    let version = review
        .version()
        .checked_add(1)
        .ok_or(DatabaseError::InvalidQuery("审核版本溢出"))?;
    let (state, status, remote) = if review.deadline_at() <= now {
        (ReviewState::Expired, Value::Null, Cancellation::Frozen)
    } else {
        let (status, remote) = provider_cancellation(review, version);
        (ReviewState::Cancelled, status.as_str().into(), remote)
    };
    let changed = transaction
        .table(REVIEWS)
        .r#where("id", "=", review.id())
        .r#where("scope_key", "=", scope)
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
                ("provider_retry_at".into(), Value::Null),
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
