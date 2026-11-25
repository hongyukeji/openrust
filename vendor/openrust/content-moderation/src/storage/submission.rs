use std::sync::Arc;

use openrust::database::{DatabaseError, Value};

use crate::{ModerationError, ModerationResult, ReviewState, VideoReview, VideoSubmission};

use super::{CHECKS, ModerationStore, REVIEWS, database_clock, database_clock_query, integer};

// 给当前最长 120 秒的供应商请求留出提交上下文时间，到期只转未知。
const SUBMISSION_LEASE_SECONDS: i64 = 150;

#[derive(Clone)]
enum Action {
    Claim,
    Recover,
    Finish(VideoSubmission),
}

impl ModerationStore {
    pub(crate) async fn claim_submission(
        &self,
        review: &VideoReview,
    ) -> ModerationResult<Option<VideoReview>> {
        self.transition(review, Action::Claim).await
    }

    pub(crate) async fn recover_submission(&self, review: &VideoReview) -> ModerationResult<()> {
        self.transition(review, Action::Recover).await?;
        Ok(())
    }

    pub(crate) async fn finish_submission(
        &self,
        claim: &VideoReview,
        outcome: VideoSubmission,
    ) -> ModerationResult<()> {
        self.transition(claim, Action::Finish(outcome)).await?;
        Ok(())
    }

    pub(crate) async fn submission_is_live(&self, claim: &VideoReview) -> ModerationResult<bool> {
        let now = database_clock(&self.connection);
        self.connection
            .table(REVIEWS)
            .use_write_pdo()
            .join(
                CHECKS,
                "moderation_reviews.id",
                "=",
                "moderation_checks.review_id",
            )
            .r#where("moderation_reviews.scope_key", "=", self.scope_key.as_str())
            .r#where("moderation_reviews.id", "=", claim.id.as_str())
            .r#where("moderation_reviews.version", "=", claim.version)
            .r#where(
                "moderation_reviews.state",
                "=",
                ReviewState::Running.as_str(),
            )
            .r#where("moderation_checks.id", "=", claim.check_id.as_str())
            .r#where("moderation_checks.submission", "=", "Submitting")
            .where_raw(
                &format!("deadline_at > {now} and submit_lease_until > {now}"),
                Vec::new(),
            )
            .exists()
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    async fn transition(
        &self,
        review: &VideoReview,
        action: Action,
    ) -> ModerationResult<Option<VideoReview>> {
        let review = Arc::new(review.clone());
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        self.connection
            .transaction(
                move |transaction| {
                    let review = review.clone();
                    let scope = scope.clone();
                    let clock = clock.clone();
                    let action = action.clone();
                    Box::pin(async move {
                        let now = transaction
                            .select_one(&clock, &[])
                            .await?
                            .and_then(|row| integer(&row, "created_at"))
                            .ok_or(DatabaseError::InvalidQuery("无法读取审核存储时钟"))?;
                        let Some(next) = next_state(&review, action, now) else {
                            return Ok(None);
                        };
                        let affected = transaction
                            .table(REVIEWS)
                            .r#where("id", "=", review.id.as_str())
                            .r#where("scope_key", "=", scope.as_str())
                            .r#where("version", "=", review.version)
                            .update(
                                [
                                    ("version".into(), next.version.into()),
                                    ("state".into(), next.state.as_str().into()),
                                ]
                                .into_iter()
                                .collect(),
                            )
                            .await?;
                        if affected == 0 {
                            return Ok(None);
                        }
                        if next.submission == review.submission
                            && next.submit_lease_until == review.submit_lease_until
                        {
                            return Ok(Some(next));
                        }
                        let remote_id = match &next.submission {
                            VideoSubmission::Submitted(id) => id.as_str().into(),
                            _ => Value::Null,
                        };
                        let failure: Value = match &next.submission {
                            VideoSubmission::Failed(reason) => serde_json::to_value(reason)
                                .ok()
                                .and_then(|value| value.as_str().map(Value::from))
                                .ok_or(DatabaseError::InvalidQuery("无法保存送审失败原因"))?,
                            _ => Value::Null,
                        };
                        let changed_check = transaction
                            .table(CHECKS)
                            .r#where("id", "=", next.check_id.as_str())
                            .r#where("review_id", "=", next.id.as_str())
                            .r#where("submission", "=", review.submission.as_str())
                            .update(
                                [
                                    ("submission".into(), next.submission.as_str().into()),
                                    (
                                        "submit_lease_until".into(),
                                        next.submit_lease_until.map_or(Value::Null, Value::from),
                                    ),
                                    ("remote_task_id".into(), remote_id),
                                    ("submit_failure".into(), failure),
                                ]
                                .into_iter()
                                .collect(),
                            )
                            .await?;
                        if changed_check != 1 {
                            return Err(DatabaseError::InvalidQuery("送审检查状态与父审核不一致"));
                        }
                        Ok(Some(next))
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }
}

fn next_state(review: &VideoReview, action: Action, now: i64) -> Option<VideoReview> {
    let expired = review.deadline_at <= now;
    let (state, submission, lease) = match action {
        Action::Claim if !expired && review.submission == VideoSubmission::Ready => (
            ReviewState::Running,
            VideoSubmission::Submitting,
            Some((now + SUBMISSION_LEASE_SECONDS).min(review.deadline_at)),
        ),
        Action::Finish(result) if review.submission == VideoSubmission::Submitting => {
            let state = if expired {
                ReviewState::Expired
            } else if matches!(result, VideoSubmission::Failed(_)) {
                ReviewState::Failed
            } else {
                ReviewState::Running
            };
            (state, result, None)
        }
        Action::Recover
            if review.submission == VideoSubmission::Submitting
                && (expired || review.submit_lease_until.is_some_and(|until| until <= now)) =>
        {
            (
                if expired {
                    ReviewState::Expired
                } else {
                    ReviewState::Running
                },
                VideoSubmission::Unknown,
                None,
            )
        }
        Action::Recover
            if expired && matches!(review.state, ReviewState::Pending | ReviewState::Running) =>
        {
            (
                ReviewState::Expired,
                if review.submission == VideoSubmission::Ready {
                    VideoSubmission::Expired
                } else {
                    review.submission.clone()
                },
                None,
            )
        }
        _ => return None,
    };
    Some(VideoReview {
        state,
        submission,
        submit_lease_until: lease,
        version: review.version.checked_add(1)?,
        ..review.clone()
    })
}
