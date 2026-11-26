use std::sync::Arc;

use openrust::database::{DatabaseError, Transaction, Value};

use super::{
    CHECKS, ModerationStore, REVIEWS, database_clock_query, decisions::DECISIONS, digest,
    groups::GROUPS, integer, late_notices::NOTICES, read_clock, tombstones,
};
use crate::{
    ModerationError, ModerationResult, ReviewState, VideoReview, VideoReviewTarget, VideoSubmission,
};

const PURGE_FORBIDDEN: &str = "内容审核清理要求已撤回且所有检查已结束";
const PURGE_CHANGED: &str = "待清理的内容审核已变化";

struct PurgeTarget {
    group: Option<String>,
    reviews: Vec<VideoReview>,
}

impl ModerationStore {
    /// 清理已撤回修订的终态审核及证据，短期保留已知供应商任务的关联摘要。
    pub async fn purge_withdrawn_video(
        &self,
        target: VideoReviewTarget<'_>,
        retain_seconds: u32,
    ) -> ModerationResult<usize> {
        if !(60..=604_800).contains(&retain_seconds) {
            return Err(ModerationError::InvalidInput("video tombstone retention"));
        }
        let Some(record) = self.find_purge_target(target).await? else {
            return Ok(0);
        };
        let first = &record.reviews[0];
        let control = self.revision_control(first.content_id(), first.content_revision());
        let record = Arc::new(record);
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        let result = self
            .connection
            .transaction(
                move |tx| {
                    let (record, control, scope, clock) = (
                        record.clone(),
                        control.clone(),
                        scope.clone(),
                        clock.clone(),
                    );
                    Box::pin(async move {
                        if control.lock(tx).await?.is_none() {
                            return Err(DatabaseError::InvalidQuery(PURGE_FORBIDDEN));
                        }
                        let expires = read_clock(tx, &clock)
                            .await?
                            .checked_add(i64::from(retain_seconds))
                            .ok_or(DatabaseError::InvalidQuery("内容审核墓碑保留时间溢出"))?;
                        for review in &record.reviews {
                            lock_terminal_review(tx, &scope, review, record.group.is_some())
                                .await?;
                        }
                        for review in &record.reviews {
                            if let VideoSubmission::Submitted(task) = review.submission() {
                                tombstones::retain(
                                    tx,
                                    &scope,
                                    review.profile().driver(),
                                    review.profile().account(),
                                    task,
                                    expires,
                                )
                                .await?;
                            }
                            remove_review(tx, &scope, review).await?;
                        }
                        if let Some(group) = &record.group {
                            remove_decisions(tx, &scope, "video_group", group).await?;
                            let changed = tx
                                .table(GROUPS)
                                .r#where("scope_key", "=", scope.as_str())
                                .where_exact_text("id", group)
                                .delete()
                                .await?;
                            if changed != 1 {
                                return Err(DatabaseError::InvalidQuery(PURGE_CHANGED));
                            }
                        }
                        Ok(record.reviews.len())
                    })
                },
                3,
            )
            .await;
        match result {
            Ok(count) => Ok(count),
            Err(DatabaseError::InvalidQuery(PURGE_FORBIDDEN)) => {
                Err(ModerationError::ReviewConflict)
            }
            Err(DatabaseError::InvalidQuery(PURGE_CHANGED))
                if self.find_purge_target(target).await?.is_none() =>
            {
                Ok(0)
            }
            Err(_) => Err(ModerationError::ReviewStorage),
        }
    }

    async fn find_purge_target(
        &self,
        target: VideoReviewTarget<'_>,
    ) -> ModerationResult<Option<PurgeTarget>> {
        match target {
            VideoReviewTarget::Single(id) => Ok(self.find(id).await?.map(|review| PurgeTarget {
                group: None,
                reviews: vec![review],
            })),
            VideoReviewTarget::Group(id) => {
                Ok(self.find_video_group(id).await?.map(|group| PurgeTarget {
                    group: Some(group.id),
                    reviews: group.checks,
                }))
            }
        }
    }
}

async fn lock_terminal_review(
    tx: &mut Transaction,
    scope: &str,
    review: &VideoReview,
    grouped: bool,
) -> Result<(), DatabaseError> {
    let row = tx
        .table(REVIEWS)
        .r#where("scope_key", "=", scope)
        .where_exact_text("id", review.id())
        .select(["state", "intent_key", "version"])
        .lock_for_update()
        .first()
        .await?
        .ok_or(DatabaseError::InvalidQuery(PURGE_CHANGED))?;
    let state = match row.get("state") {
        Some(Value::Text(state)) => {
            ReviewState::parse(state).map_err(|_| DatabaseError::InvalidQuery(PURGE_CHANGED))?
        }
        _ => return Err(DatabaseError::InvalidQuery(PURGE_CHANGED)),
    };
    if matches!(state, ReviewState::Pending | ReviewState::Running) {
        return Err(DatabaseError::InvalidQuery(PURGE_FORBIDDEN));
    }
    if !grouped
        && let Some(Value::Text(intent)) = row.get("intent_key")
        && *intent == digest(&format!("group-check:{}", review.id()))
    {
        return Err(DatabaseError::InvalidQuery(PURGE_FORBIDDEN));
    }
    if integer(&row, "version") != Some(review.version()) || state != review.state() {
        return Err(DatabaseError::InvalidQuery(PURGE_CHANGED));
    }
    Ok(())
}

async fn remove_review(
    tx: &mut Transaction,
    scope: &str,
    review: &VideoReview,
) -> Result<(), DatabaseError> {
    remove_decisions(tx, scope, "video", review.id()).await?;
    tx.table(NOTICES)
        .r#where("scope_key", "=", scope)
        .where_exact_text("review_id", review.id())
        .delete()
        .await?;
    let removed = tx
        .table(CHECKS)
        .where_exact_text("id", review.check_id())
        .where_exact_text("review_id", review.id())
        .delete()
        .await?;
    if removed != 1 {
        return Err(DatabaseError::InvalidQuery(PURGE_CHANGED));
    }
    let removed = tx
        .table(REVIEWS)
        .r#where("scope_key", "=", scope)
        .where_exact_text("id", review.id())
        .delete()
        .await?;
    if removed != 1 {
        return Err(DatabaseError::InvalidQuery(PURGE_CHANGED));
    }
    Ok(())
}

async fn remove_decisions(
    tx: &mut Transaction,
    scope: &str,
    kind: &str,
    id: &str,
) -> Result<(), DatabaseError> {
    tx.table(DECISIONS)
        .r#where("scope_key", "=", scope)
        .r#where("review_kind", "=", kind)
        .where_exact_text("review_id", id)
        .delete()
        .await?;
    Ok(())
}
