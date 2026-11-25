use crate::{
    Decision, ModerationError, ModerationResult, ProfileRevision, ReviewState, StoredMedia,
    VideoReview, VideoReviewRequest,
};

pub(crate) const MAX_GROUP_CHECKS: usize = 8;

/// 同一内容修订的多个显式检查，由调用方固定每项配置修订。
pub struct VideoReviewGroupRequest {
    pub(crate) checks: Vec<VideoReviewRequest>,
}

impl VideoReviewGroupRequest {
    pub fn new(
        intent: &str,
        content_id: &str,
        content_revision: &str,
        source: StoredMedia,
        profiles: Vec<ProfileRevision>,
        timeout_seconds: u32,
    ) -> ModerationResult<Self> {
        if !(2..=MAX_GROUP_CHECKS).contains(&profiles.len())
            || profiles.iter().enumerate().any(|(index, profile)| {
                profile.required_modalities().is_empty() || profiles[..index].contains(profile)
            })
        {
            return Err(ModerationError::InvalidInput("review group checks"));
        }
        let checks = profiles
            .into_iter()
            .map(|profile| {
                VideoReviewRequest::new(
                    intent,
                    content_id,
                    content_revision,
                    source.clone(),
                    profile,
                    timeout_seconds,
                )
            })
            .collect::<ModerationResult<Vec<_>>>()?;
        Ok(Self { checks })
    }
}

/// 按固定检查顺序读取父审核及各检查的已持久化状态。
pub struct VideoReviewGroup {
    pub(crate) id: String,
    pub(crate) checks: Vec<VideoReview>,
}

impl VideoReviewGroup {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn checks(&self) -> &[VideoReview] {
        &self.checks
    }

    pub fn state(&self) -> ReviewState {
        let states = || self.checks.iter().map(VideoReview::state);
        for terminal in [
            ReviewState::Failed,
            ReviewState::Cancelled,
            ReviewState::Expired,
        ] {
            if states().any(|state| state == terminal) {
                return terminal;
            }
        }
        if states().all(|state| state == ReviewState::Succeeded) {
            ReviewState::Succeeded
        } else if states().all(|state| state == ReviewState::Pending) {
            ReviewState::Pending
        } else {
            ReviewState::Running
        }
    }

    /// 所有检查具备完整覆盖后，取各项结论中风险最高的一项。
    pub fn decision(&self) -> Option<Decision> {
        self.checks
            .iter()
            .try_fold(Decision::Allow, |result, check| {
                match (result, check.decision()?) {
                    (Decision::Block, _) | (_, Decision::Block) => Some(Decision::Block),
                    (Decision::Review, _) | (_, Decision::Review) => Some(Decision::Review),
                    (Decision::Allow, Decision::Allow) => Some(Decision::Allow),
                }
            })
    }
}
