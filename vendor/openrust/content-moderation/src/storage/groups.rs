use std::collections::BTreeMap;

use openrust::database::{DatabaseError, Row, Transaction, Value};

use super::{
    ModerationStore, REVIEWS, ReviewIds, database_clock_query, decode, digest, new_id, read_clock,
    segment_record, write_check,
};
use crate::{
    Modality, ModerationError, ModerationResult, VideoReview, VideoReviewGroup,
    VideoReviewGroupRequest,
    credentials::valid_identifier,
    review_group::MAX_GROUP_CHECKS,
    segment_plan::{MAX_VIDEO_SEGMENTS, SegmentPlan},
};
use std::sync::Arc;

const GROUPS: &str = "moderation_review_groups";

impl ModerationStore {
    /// 同一数据库事务登记父审核和所有必检任务。
    pub async fn stage_video_group(
        &self,
        request: &VideoReviewGroupRequest,
    ) -> ModerationResult<VideoReviewGroup> {
        let intent = digest(&request.checks[0].intent);
        if let Some(group) = self.read_group("intent_key", &intent).await? {
            return existing(group, request);
        }
        let id = new_id()?;
        let ids = request
            .checks
            .iter()
            .map(|_| ReviewIds::generate())
            .collect::<ModerationResult<Vec<_>>>()?;
        let scope = self.scope_key.clone();
        let clock = database_clock_query(&self.connection);
        let checks = Arc::new(request.checks.clone());
        let members = Arc::new(ids);
        let plan = Arc::new(
            request
                .segments
                .as_ref()
                .map(segment_record::encode)
                .transpose()?,
        );
        let group_id = id.clone();
        let group_intent = intent.clone();
        let saved = self
            .connection
            .transaction(
                move |transaction| {
                    let (id, intent, scope, clock) = (
                        group_id.clone(),
                        group_intent.clone(),
                        scope.clone(),
                        clock.clone(),
                    );
                    let (checks, members, plan) = (checks.clone(), members.clone(), plan.clone());
                    Box::pin(async move {
                        let now = read_clock(transaction, &clock).await?;
                        insert_group(transaction, &id, &scope, &intent, &members, plan.as_deref())
                            .await?;
                        for (request, ids) in checks.iter().zip(members.iter()) {
                            let child_intent = digest(&format!("group-check:{}", ids.review));
                            write_check(transaction, ids, &scope, &child_intent, request, now)
                                .await?;
                        }
                        Ok(())
                    })
                },
                3,
            )
            .await;
        if saved.is_ok() {
            return self
                .find_video_group(&id)
                .await?
                .ok_or(ModerationError::ReviewStorage);
        }
        self.read_group("intent_key", &intent)
            .await?
            .ok_or(ModerationError::ReviewStorage)
            .and_then(|group| existing(group, request))
    }

    pub async fn find_video_group(&self, id: &str) -> ModerationResult<Option<VideoReviewGroup>> {
        if id.len() != 36 || !valid_identifier(id) {
            return Ok(None);
        }
        self.read_group("id", id).await
    }

    async fn read_group(
        &self,
        column: &str,
        value: &str,
    ) -> ModerationResult<Option<VideoReviewGroup>> {
        let row = self
            .connection
            .table(GROUPS)
            .use_write_pdo()
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_exact_text(column, value)
            .first()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id = text(&row, "id")?;
        let serialized = text(&row, "member_ids")?;
        let segments = match row.get("segment_plan") {
            Some(Value::Null) => None,
            Some(Value::Text(plan)) => Some(segment_record::decode(plan)?),
            _ => return Err(ModerationError::ReviewStorage),
        };
        let maximum = if segments.is_some() {
            MAX_VIDEO_SEGMENTS
        } else {
            MAX_GROUP_CHECKS
        };
        if serialized.len() > maximum * 40 {
            return Err(ModerationError::ReviewStorage);
        }
        let members: Vec<String> =
            serde_json::from_str(serialized).map_err(|_| ModerationError::ReviewStorage)?;
        if !(2..=maximum).contains(&members.len()) {
            return Err(ModerationError::ReviewStorage);
        }
        let rows = self
            .review_rows()
            .where_in(
                &format!("{REVIEWS}.id"),
                members
                    .iter()
                    .map(|member| member.as_str().into())
                    .collect(),
            )
            .limit(maximum as u64)
            .get()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        let (mut ordered, mut checks) = (BTreeMap::new(), Vec::with_capacity(members.len()));
        for row in rows {
            let review = decode(row)?;
            ordered.insert(review.id().to_owned(), review);
        }
        for member in members {
            checks.push(
                ordered
                    .remove(&member)
                    .ok_or(ModerationError::ReviewStorage)?,
            );
        }
        let original = &checks[0];
        if checks.iter().any(|check| {
            check.content_id() != original.content_id()
                || check.content_revision() != original.content_revision()
                || check.created_at() != original.created_at()
                || check.deadline_at() != original.deadline_at()
        }) || !valid_scope(&checks, segments.as_ref())
        {
            return Err(ModerationError::ReviewStorage);
        }
        Ok(Some(VideoReviewGroup {
            id: id.to_owned(),
            checks,
            segments,
        }))
    }
}

fn valid_scope(checks: &[VideoReview], segments: Option<&SegmentPlan>) -> bool {
    let first = &checks[0];
    match segments {
        None => checks.iter().all(|check| check.source() == first.source()),
        Some(plan) => {
            first.profile().required_modalities().len() == 1
                && first
                    .profile()
                    .required_modalities()
                    .contains(&Modality::Speech)
                && checks
                    .iter()
                    .all(|check| check.profile() == first.profile())
                && plan.covers(checks.iter().map(VideoReview::source))
        }
    }
}

fn text<'a>(row: &'a Row, field: &str) -> ModerationResult<&'a str> {
    match row.get(field) {
        Some(Value::Text(value)) => Ok(value.as_str()),
        _ => Err(ModerationError::ReviewStorage),
    }
}

fn existing(
    group: VideoReviewGroup,
    request: &VideoReviewGroupRequest,
) -> ModerationResult<VideoReviewGroup> {
    if group.segments == request.segments
        && group.checks.len() == request.checks.len()
        && group
            .checks
            .iter()
            .zip(&request.checks)
            .all(|(stored, requested)| stored.matches(requested))
    {
        Ok(group)
    } else {
        Err(ModerationError::ReviewConflict)
    }
}

async fn insert_group(
    transaction: &mut Transaction,
    id: &str,
    scope: &str,
    intent: &str,
    members: &[ReviewIds],
    plan: Option<&str>,
) -> Result<(), DatabaseError> {
    let ids: Vec<_> = members
        .iter()
        .map(|member| member.review.as_str())
        .collect();
    let ids = serde_json::to_string(&ids)
        .map_err(|_| DatabaseError::InvalidQuery("无法序列化复合审核检查"))?;
    transaction
        .table(GROUPS)
        .insert(&[[
            ("id".into(), id.into()),
            ("scope_key".into(), scope.into()),
            ("intent_key".into(), intent.into()),
            ("member_ids".into(), ids.into()),
            ("segment_plan".into(), plan.map_or(Value::Null, Value::from)),
        ]
        .into_iter()
        .collect()])
        .await
        .map(|_| ())
}
