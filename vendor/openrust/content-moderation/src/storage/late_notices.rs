use openrust::database::{DatabaseError, Row, Transaction, Value};

use super::{CHECKS, ModerationStore, REVIEWS, digest, integer};
#[cfg(feature = "webhooks")]
use super::{database_clock_query, read_clock};
use crate::{
    LateVideoNotice, LateVideoNoticeKind, ModerationError, ModerationResult, ReviewState,
    VideoReview, credentials::valid_identifier,
};

pub(super) const NOTICES: &str = "moderation_late_notices";

impl ModerationStore {
    /// 返回作用域内每个来源的首次迟到观察，诊断不包含远端响应正文。
    pub async fn late_video_notices(&self, id: &str) -> ModerationResult<Vec<LateVideoNotice>> {
        if id.len() != 36 || !valid_identifier(id) {
            return Ok(Vec::new());
        }
        let rows = self
            .connection
            .table(NOTICES)
            .use_write_pdo()
            .r#where("scope_key", "=", self.scope_key.as_str())
            .where_exact_text("review_id", id)
            .order_by("observed_at", "asc")
            .order_by("kind", "asc")
            .limit(4)
            .get()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        if rows.len() > 3 {
            return Err(ModerationError::ReviewStorage);
        }
        rows.into_iter().map(decode).collect()
    }

    #[cfg(feature = "webhooks")]
    pub(crate) async fn note_late_callback(
        &self,
        review: &VideoReview,
        receipt: &str,
    ) -> ModerationResult<()> {
        if receipt.len() != 36 || !valid_identifier(receipt) {
            return Err(ModerationError::ReviewStorage);
        }
        let review = std::sync::Arc::new(review.clone());
        let (scope, receipt) = (self.scope_key.clone(), receipt.to_owned());
        let clock = database_clock_query(&self.connection);
        self.connection
            .transaction(
                move |tx| {
                    let (scope, receipt, clock, review) = (
                        scope.clone(),
                        receipt.clone(),
                        clock.clone(),
                        review.clone(),
                    );
                    Box::pin(async move {
                        let now = read_clock(tx, &clock).await?;
                        note_late(
                            tx,
                            &scope,
                            &review,
                            LateVideoNoticeKind::SignedCallback,
                            Some(&receipt),
                            now,
                        )
                        .await
                    })
                },
                3,
            )
            .await
            .map_err(|_| ModerationError::ReviewStorage)
    }
}

/// 由真实供应商结果调用，只在当前记录已经隔离该结果时留存诊断。
pub(super) async fn note_late(
    tx: &mut Transaction,
    scope: &str,
    review: &VideoReview,
    kind: LateVideoNoticeKind,
    receipt: Option<&str>,
    now: i64,
) -> Result<(), DatabaseError> {
    let current = tx
        .table(REVIEWS)
        .join(
            CHECKS,
            "moderation_reviews.id",
            "=",
            "moderation_checks.review_id",
        )
        .r#where("moderation_reviews.scope_key", "=", scope)
        .r#where("moderation_reviews.id", "=", review.id())
        .r#where("moderation_checks.id", "=", review.check_id())
        .select(["state", "submission", "deadline_at"])
        .lock_for_update()
        .first()
        .await?;
    let Some(row) = current else {
        return Ok(());
    };
    let passed_deadline = integer(&row, "deadline_at").is_some_and(|deadline| deadline <= now);
    let state = match (row.get("state"), row.get("submission")) {
        (Some(Value::Text(state)), Some(Value::Text(submission)))
            if passed_deadline
                || matches!(state.as_str(), "Cancelled" | "Expired")
                || matches!(kind, LateVideoNoticeKind::Submission)
                    && state == "Running"
                    && submission == "Unknown" =>
        {
            state
        }
        _ => return Ok(()),
    };
    let id = digest(&format!("{}\n{}", review.check_id(), kind.as_str()));
    tx.table(NOTICES)
        .upsert(
            &[[
                ("id".into(), id.into()),
                ("scope_key".into(), scope.into()),
                ("review_id".into(), review.id().into()),
                ("kind".into(), kind.as_str().into()),
                ("review_state".into(), state.as_str().into()),
                ("observed_at".into(), now.into()),
                (
                    "receipt_id".into(),
                    receipt.map_or(Value::Null, Value::from),
                ),
            ]
            .into_iter()
            .collect()],
            &["id"],
            Some(&["id"]),
        )
        .await?;
    Ok(())
}

fn decode(row: Row) -> ModerationResult<LateVideoNotice> {
    let text = |column| match row.get(column) {
        Some(Value::Text(value)) => Ok(value.as_str()),
        _ => Err(ModerationError::ReviewStorage),
    };
    let receipt_id = match row.get("receipt_id") {
        Some(Value::Null) => None,
        Some(Value::Text(id)) if id.len() == 36 && valid_identifier(id) => Some(id.to_owned()),
        _ => return Err(ModerationError::ReviewStorage),
    };
    Ok(LateVideoNotice {
        kind: LateVideoNoticeKind::parse(text("kind")?)?,
        state: ReviewState::parse(text("review_state")?)?,
        observed_at: integer(&row, "observed_at").ok_or(ModerationError::ReviewStorage)?,
        receipt_id,
    })
}
