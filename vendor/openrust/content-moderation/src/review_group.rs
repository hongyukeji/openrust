use crate::{
    Decision, Modality, ModerationError, ModerationResult, ProfileRevision, ReviewState,
    StoredMedia, VideoReview, VideoReviewRequest, VideoReviewSegment, segment_plan::SegmentPlan,
};

pub(crate) const MAX_GROUP_CHECKS: usize = 8;

/// 同一内容修订的多个显式检查，由调用方固定每项配置修订。
pub struct VideoReviewGroupRequest {
    pub(crate) checks: Vec<VideoReviewRequest>,
    pub(crate) segments: Option<SegmentPlan>,
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
        Ok(Self {
            checks,
            segments: None,
        })
    }

    /// 保存由应用验证的语音片段及原素材时间线。重叠片段可以提供边界上下文。
    pub fn for_speech_segments(
        intent: &str,
        content_id: &str,
        content_revision: &str,
        original: StoredMedia,
        profile: ProfileRevision,
        segments: Vec<VideoReviewSegment>,
        timeout_seconds: u32,
    ) -> ModerationResult<Self> {
        if profile.required_modalities().len() != 1
            || !profile.required_modalities().contains(&Modality::Speech)
            || segments.len() > crate::segment_plan::MAX_VIDEO_SEGMENTS
        {
            return Err(ModerationError::InvalidInput("speech video segment plan"));
        }
        let plan = SegmentPlan {
            original,
            start_offsets: segments.iter().map(|segment| segment.start_ms).collect(),
        };
        if !plan.covers(segments.iter().map(|segment| &segment.source)) {
            return Err(ModerationError::InvalidInput("speech video segment plan"));
        }
        let checks = segments
            .into_iter()
            .map(|segment| {
                VideoReviewRequest::new(
                    intent,
                    content_id,
                    content_revision,
                    segment.source,
                    profile.clone(),
                    timeout_seconds,
                )
            })
            .collect::<ModerationResult<Vec<_>>>()?;
        Ok(Self {
            checks,
            segments: Some(plan),
        })
    }
}

/// 按固定检查顺序读取父审核及各检查的已持久化状态。
pub struct VideoReviewGroup {
    pub(crate) id: String,
    pub(crate) checks: Vec<VideoReview>,
    pub(crate) segments: Option<SegmentPlan>,
}

impl VideoReviewGroup {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn checks(&self) -> &[VideoReview] {
        &self.checks
    }

    pub fn source(&self) -> &StoredMedia {
        match &self.segments {
            Some(plan) => &plan.original,
            None => self.checks[0].source(),
        }
    }

    /// 在原素材时间线上返回第 index 个已保存片段的起止毫秒。
    pub fn segment_window_ms(&self, index: usize) -> Option<(u32, u32)> {
        let start = *self.segments.as_ref()?.start_offsets.get(index)?;
        let duration = self.checks.get(index)?.source().video_duration_ms()?;
        Some((start, start.checked_add(duration)?))
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
        if self.checks[0].profile().driver() == "sightengine"
            && self
                .segments
                .as_ref()
                .is_some_and(|plan| plan.original.video_audio_track_count() != Some(1))
        {
            return None;
        }
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
