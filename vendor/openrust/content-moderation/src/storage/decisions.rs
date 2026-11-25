use std::sync::Arc;

use openrust::database::{DatabaseError, Row, Value, query::Builder};

use super::{ModerationStore, database_clock_query, integer, read_clock, withdrawals::WITHDRAWN};
use crate::{
    Decision, ManualReviewDecision, ModerationError, ModerationResult, ReviewDecisionRequest,
    VideoReviewTarget, credentials::valid_identifier, review::valid_content_identifier,
};

const DECISIONS: &str = "moderation_review_decisions";

impl ModerationStore {
    pub(crate) async fn latest_decision(
        &self,
        target: VideoReviewTarget<'_>,
    ) -> ModerationResult<Option<ManualReviewDecision>> {
        let row = self
            .decision_rows(target)
            .order_by_desc("revision")
            .first()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        row.map(decode).transpose()
    }

    pub(crate) async fn append_decision(
        &self,
        target: VideoReviewTarget<'_>,
        request: &ReviewDecisionRequest,
    ) -> ModerationResult<ManualReviewDecision> {
        let (kind, id) = target.parts();
        let revision = request.expected_revision + 1;
        let decision = match request.decision {
            Decision::Allow => "allow",
            Decision::Review => "review",
            Decision::Block => "block",
        };
        let fields = Arc::new([
            ("scope_key", Value::from(self.scope_key.as_str())),
            ("review_kind", kind.into()),
            ("review_id", id.into()),
            ("revision", revision.into()),
            ("decision", decision.into()),
            ("actor", request.actor.as_str().into()),
            ("reason", request.reason.as_str().into()),
        ]);
        let clock = database_clock_query(&self.connection);
        let control = self.revision_control(&request.content_id, &request.content_revision);
        let written = self
            .connection
            .transaction(
                move |transaction| {
                    let (fields, clock, control) = (fields.clone(), clock.clone(), control.clone());
                    Box::pin(async move {
                        if control.lock(transaction).await?.is_some() {
                            return Err(DatabaseError::InvalidQuery(WITHDRAWN));
                        }
                        let now = read_clock(transaction, &clock).await?;
                        let values = fields
                            .iter()
                            .cloned()
                            .chain([("decided_at", now.into())])
                            .map(|(key, value)| (key.into(), value))
                            .collect();
                        transaction.table(DECISIONS).insert(&[values]).await?;
                        Ok(now)
                    })
                },
                3,
            )
            .await;
        match written {
            Ok(decided_at) => Ok(ManualReviewDecision {
                revision,
                decision: request.decision,
                actor: request.actor.clone(),
                reason: request.reason.clone(),
                decided_at,
            }),
            Err(DatabaseError::InvalidQuery(WITHDRAWN)) => Err(ModerationError::ReviewConflict),
            Err(_) => {
                if self
                    .latest_decision(target)
                    .await?
                    .is_some_and(|existing| existing.revision() >= revision)
                {
                    Err(ModerationError::ReviewConflict)
                } else {
                    Err(ModerationError::ReviewStorage)
                }
            }
        }
    }

    fn decision_rows(&self, target: VideoReviewTarget<'_>) -> Builder<'_> {
        let (kind, id) = target.parts();
        self.connection
            .table(DECISIONS)
            .use_write_pdo()
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("review_kind", "=", kind)
            .where_exact_text("review_id", id)
    }
}

fn decode(row: Row) -> ModerationResult<ManualReviewDecision> {
    let invalid = || ModerationError::ReviewStorage;
    let text = |field| match row.get(field) {
        Some(Value::Text(value)) => Ok(value.as_str()),
        _ => Err(invalid()),
    };
    let revision = integer(&row, "revision")
        .filter(|revision| *revision > 0)
        .ok_or_else(invalid)?;
    let decided_at = integer(&row, "decided_at").ok_or_else(invalid)?;
    let decision = match text("decision")? {
        "allow" => Decision::Allow,
        "review" => Decision::Review,
        "block" => Decision::Block,
        _ => return Err(invalid()),
    };
    let (actor, reason) = (text("actor")?, text("reason")?);
    if !valid_identifier(actor) || !valid_content_identifier(reason, 1024) {
        return Err(invalid());
    }
    Ok(ManualReviewDecision {
        revision,
        decision,
        actor: actor.to_owned(),
        reason: reason.to_owned(),
        decided_at,
    })
}
