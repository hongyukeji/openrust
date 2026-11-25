use crate::{
    Decision, ModerationError, ModerationResult, credentials::valid_identifier,
    review::valid_content_identifier,
};

/// 应用按自身审核策略选择完整的审核单元。
#[derive(Clone, Copy)]
pub enum VideoReviewTarget<'a> {
    Single(&'a str),
    Group(&'a str),
}

impl<'a> VideoReviewTarget<'a> {
    pub(crate) fn parts(self) -> (&'static str, &'a str) {
        match self {
            Self::Single(id) => ("video", id),
            Self::Group(id) => ("video_group", id),
        }
    }
}

/// 已完成身份鉴别与授权的应用提交人工裁决，修订 0 表示自动结论。
pub struct ReviewDecisionRequest {
    pub(crate) content_id: String,
    pub(crate) content_revision: String,
    pub(crate) expected_revision: i64,
    pub(crate) decision: Decision,
    pub(crate) actor: String,
    pub(crate) reason: String,
}

impl ReviewDecisionRequest {
    pub fn new(
        content_id: &str,
        content_revision: &str,
        expected_revision: i64,
        decision: Decision,
        actor: &str,
        reason: &str,
    ) -> ModerationResult<Self> {
        if !valid_content_identifier(content_id, 256)
            || !valid_content_identifier(content_revision, 128)
            || !(0..i64::MAX).contains(&expected_revision)
            || !valid_identifier(actor)
            || !valid_content_identifier(reason, 1024)
        {
            return Err(ModerationError::InvalidInput("review decision"));
        }
        Ok(Self {
            content_id: content_id.to_owned(),
            content_revision: content_revision.to_owned(),
            expected_revision,
            decision,
            actor: actor.to_owned(),
            reason: reason.to_owned(),
        })
    }
}

pub enum ReviewDecision {
    Automatic(Decision),
    Manual(ManualReviewDecision),
}

impl ReviewDecision {
    pub fn decision(&self) -> Decision {
        match self {
            Self::Automatic(decision) => *decision,
            Self::Manual(decision) => decision.decision(),
        }
    }

    pub fn revision(&self) -> i64 {
        match self {
            Self::Automatic(_) => 0,
            Self::Manual(decision) => decision.revision(),
        }
    }
}

/// 审核决定的有限审计元数据，不默认进入日志。
pub struct ManualReviewDecision {
    pub(crate) revision: i64,
    pub(crate) decision: Decision,
    pub(crate) actor: String,
    pub(crate) reason: String,
    pub(crate) decided_at: i64,
}

impl ManualReviewDecision {
    pub fn decision(&self) -> Decision {
        self.decision
    }

    pub fn revision(&self) -> i64 {
        self.revision
    }

    pub fn actor(&self) -> &str {
        &self.actor
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn decided_at(&self) -> i64 {
        self.decided_at
    }
}
