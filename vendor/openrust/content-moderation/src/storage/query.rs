use std::sync::Arc;

use openrust::database::{DatabaseError, Value};

use crate::{
    ModerationError, ModerationResult, QueryFailure, ReviewState, VideoReview, VideoSubmission,
    review::QueryOutcome,
};

use super::{
    CHECKS, ModerationStore, REVIEWS, assessment_record, database_clock, database_clock_query,
    integer,
};

const QUERY_LEASE_SECONDS: i64 = 150;

#[derive(Clone)]
enum Action {
    Claim,
    Wait(Option<QueryFailure>),
    Stop(ReviewState, Option<QueryFailure>, Option<String>),
}

impl ModerationStore {
    pub(crate) async fn claim_query(
        &self,
        review: &VideoReview,
    ) -> ModerationResult<Option<VideoReview>> {
        self.transition_query(review, Action::Claim).await
    }

    pub(crate) async fn finish_query(
        &self,
        claim: &VideoReview,
        outcome: QueryOutcome,
    ) -> ModerationResult<()> {
        let action = match outcome {
            QueryOutcome::Waiting => Action::Wait(None),
            QueryOutcome::Retry(failure) => Action::Wait(Some(failure)),
            QueryOutcome::Stop {
                state,
                failure,
                assessment,
            } => {
                match assessment
                    .as_ref()
                    .map(assessment_record::encode)
                    .transpose()
                {
                    Ok(encoded) => Action::Stop(state, failure, encoded),
                    Err(_) => Action::Stop(ReviewState::Failed, Some(QueryFailure::Protocol), None),
                }
            }
        };
        self.transition_query(claim, action).await?;
        Ok(())
    }

    pub(crate) async fn query_is_live(&self, claim: &VideoReview) -> ModerationResult<bool> {
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
            .r#where("moderation_checks.submission", "=", "Submitted")
            .where_raw(
                &format!("deadline_at > {now} and query_lease_until > {now}"),
                Vec::new(),
            )
            .exists()
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }

    async fn transition_query(
        &self,
        review: &VideoReview,
        action: Action,
    ) -> ModerationResult<Option<VideoReview>> {
        if review.state != ReviewState::Running
            || !matches!(review.submission, VideoSubmission::Submitted(_))
        {
            return Ok(None);
        }
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        let review = Arc::new(review.clone());
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
                        let Some((next, assessment)) = plan_query(&review, action, now) else {
                            return Ok(None);
                        };
                        let changed = transaction
                            .table(REVIEWS)
                            .r#where("id", "=", next.id.as_str())
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
                        if changed == 0 {
                            return Ok(None);
                        }
                        if next.state == ReviewState::Expired && review.query_lease_until.is_none()
                        {
                            return Ok(None);
                        }
                        let failure = match next.query_failure {
                            Some(failure) => serde_json::to_value(failure)
                                .ok()
                                .and_then(|value| value.as_str().map(Value::from))
                                .ok_or(DatabaseError::InvalidQuery("无法保存查询失败原因"))?,
                            None => Value::Null,
                        };
                        let changed_check = transaction
                            .table(CHECKS)
                            .r#where("id", "=", next.check_id.as_str())
                            .r#where("review_id", "=", next.id.as_str())
                            .r#where("submission", "=", "Submitted")
                            .update(
                                [
                                    ("available_at".into(), next.available_at.into()),
                                    (
                                        "query_attempts".into(),
                                        i64::from(next.query_attempts).into(),
                                    ),
                                    (
                                        "query_lease_until".into(),
                                        next.query_lease_until.map_or(Value::Null, Value::from),
                                    ),
                                    ("query_failure".into(), failure),
                                    ("dispatch_lease_until".into(), Value::Null),
                                    (
                                        "assessment".into(),
                                        assessment.map_or(Value::Null, Value::from),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            )
                            .await?;
                        if changed_check != 1 {
                            return Err(DatabaseError::InvalidQuery("查单检查状态与父审核不一致"));
                        }
                        Ok((next.query_lease_until.is_some()).then_some(next))
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }
}

fn plan_query(
    review: &VideoReview,
    action: Action,
    now: i64,
) -> Option<(VideoReview, Option<String>)> {
    let mut next = review.clone();
    next.version = review.version.checked_add(1)?;
    if review.deadline_at <= now {
        next.state = ReviewState::Expired;
        next.query_lease_until = None;
        return Some((next, None));
    }
    let policy = review.profile.query;
    let budget = review.query_attempts >= policy.max_attempts;
    if !matches!(action, Action::Claim) {
        review.query_lease_until?;
    }
    match action {
        Action::Claim => {
            if review.available_at > now
                || review
                    .query_lease_until
                    .is_some_and(|deadline| deadline > now)
            {
                return None;
            }
            if budget {
                next.state = ReviewState::Failed;
                next.query_failure = Some(QueryFailure::BudgetExhausted);
                next.query_lease_until = None;
            } else {
                next.query_attempts += 1;
                next.query_lease_until = Some((now + QUERY_LEASE_SECONDS).min(review.deadline_at));
            }
        }
        Action::Wait(failure) => {
            next.query_lease_until = None;
            if budget {
                next.state = ReviewState::Failed;
                next.query_failure = Some(QueryFailure::BudgetExhausted);
            } else {
                let delay = if failure.is_some() {
                    policy.retry_delay(review.query_attempts, &review.id)
                } else {
                    policy.interval()
                };
                next.available_at = (now + delay).min(review.deadline_at);
                next.query_failure = failure;
            }
        }
        Action::Stop(state, failure, record) => {
            next.state = state;
            next.query_failure = failure;
            next.query_lease_until = None;
            return Some((next, record));
        }
    }
    Some((next, None))
}
