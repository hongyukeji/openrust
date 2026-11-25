use crate::{
    Assessment, Coverage, Decision, ModerationError, ModerationResult, ProfileRevision,
    StoredMedia, TaskId, credentials::valid_identifier,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReviewState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

impl ReviewState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Expired => "Expired",
        }
    }

    pub(crate) fn parse(value: &str) -> ModerationResult<Self> {
        match value {
            "Pending" => Ok(Self::Pending),
            "Running" => Ok(Self::Running),
            "Succeeded" => Ok(Self::Succeeded),
            "Failed" => Ok(Self::Failed),
            "Cancelled" => Ok(Self::Cancelled),
            "Expired" => Ok(Self::Expired),
            _ => Err(ModerationError::ReviewStorage),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryFailure {
    BudgetExhausted,
    IncompleteCoverage,
    RevisionUnavailable,
    RevisionChanged,
    Configuration,
    Unsupported,
    Unauthorized,
    Rejected,
    Throttled,
    Timeout,
    Protocol,
    ServiceUnavailable,
    ProviderFailed,
}

pub(crate) enum QueryOutcome {
    Waiting,
    Retry(QueryFailure),
    Stop {
        state: ReviewState,
        failure: Option<QueryFailure>,
        assessment: Option<Assessment>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionFailure {
    RevisionUnavailable,
    RevisionChanged,
    Configuration,
    Unsupported,
    Input,
    Media,
    Unauthorized,
    Rejected,
    Throttled,
}

#[derive(Clone, Eq, PartialEq)]
pub enum VideoSubmission {
    Ready,
    Submitting,
    Submitted(TaskId),
    Unknown,
    Failed(SubmissionFailure),
    Expired,
}

impl VideoSubmission {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Submitting => "Submitting",
            Self::Submitted(_) => "Submitted",
            Self::Unknown => "Unknown",
            Self::Failed(_) => "Failed",
            Self::Expired => "Expired",
        }
    }
}

/// 一次送审的业务归属、内容修订及不可变素材。
#[derive(Clone, Eq, PartialEq)]
pub struct VideoReviewRequest {
    pub(crate) intent: String,
    pub(crate) content_id: String,
    pub(crate) content_revision: String,
    pub(crate) source: StoredMedia,
    pub(crate) profile: ProfileRevision,
    pub(crate) timeout_seconds: u32,
}

impl VideoReviewRequest {
    pub fn new(
        intent: &str,
        content_id: &str,
        content_revision: &str,
        source: StoredMedia,
        profile: ProfileRevision,
        timeout_seconds: u32,
    ) -> ModerationResult<Self> {
        if !valid_identifier(intent) {
            return Err(ModerationError::InvalidInput("review intent"));
        }
        if !valid_content_identifier(content_id, 256)
            || !valid_content_identifier(content_revision, 128)
        {
            return Err(ModerationError::InvalidInput("content ID or revision"));
        }
        if !(1..=86_400).contains(&timeout_seconds) {
            return Err(ModerationError::InvalidInput("review timeout"));
        }
        if !profile.supported() {
            return Err(ModerationError::ProfileRevisionChanged);
        }
        Ok(Self {
            intent: intent.into(),
            content_id: content_id.into(),
            content_revision: content_revision.into(),
            source,
            profile,
            timeout_seconds,
        })
    }
}

fn valid_content_identifier(value: &str, max_bytes: usize) -> bool {
    value.len() <= max_bytes && !value.trim().is_empty() && !value.chars().any(char::is_control)
}

/// 已持久化的父审核和点播子检查。
#[derive(Clone)]
pub struct VideoReview {
    pub(crate) id: String,
    pub(crate) check_id: String,
    pub(crate) content_id: String,
    pub(crate) content_revision: String,
    pub(crate) source: StoredMedia,
    pub(crate) profile: ProfileRevision,
    pub(crate) created_at: i64,
    pub(crate) deadline_at: i64,
    pub(crate) version: i64,
    pub(crate) state: ReviewState,
    pub(crate) submission: VideoSubmission,
    pub(crate) submit_lease_until: Option<i64>,
    pub(crate) available_at: i64,
    pub(crate) query_attempts: u16,
    pub(crate) query_lease_until: Option<i64>,
    pub(crate) query_failure: Option<QueryFailure>,
    pub(crate) assessment: Option<Assessment>,
    pub(crate) confirmation_callback_id: Option<String>,
    pub(crate) lookup_hint_id: Option<String>,
}

impl VideoReview {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn check_id(&self) -> &str {
        &self.check_id
    }
    pub fn content_id(&self) -> &str {
        &self.content_id
    }
    pub fn content_revision(&self) -> &str {
        &self.content_revision
    }
    pub fn source(&self) -> &StoredMedia {
        &self.source
    }
    pub fn profile(&self) -> &ProfileRevision {
        &self.profile
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn deadline_at(&self) -> i64 {
        self.deadline_at
    }
    pub fn version(&self) -> i64 {
        self.version
    }

    pub fn state(&self) -> ReviewState {
        self.state
    }

    pub fn submission(&self) -> &VideoSubmission {
        &self.submission
    }

    pub fn query_attempts(&self) -> u16 {
        self.query_attempts
    }

    pub fn next_query_at(&self) -> Option<i64> {
        if self.state != ReviewState::Running
            || !matches!(self.submission, VideoSubmission::Submitted(_))
        {
            return None;
        }
        Some(
            self.available_at
                .max(self.query_lease_until.unwrap_or(self.available_at)),
        )
    }

    pub fn query_failure(&self) -> Option<QueryFailure> {
        self.query_failure
    }

    pub fn assessment(&self) -> Option<&Assessment> {
        self.assessment.as_ref()
    }

    pub fn confirmation_callback_id(&self) -> Option<&str> {
        self.confirmation_callback_id.as_deref()
    }

    /// 未认证查单提示的记录 ID。其出现仅表示请求提前查单。
    pub fn lookup_hint_id(&self) -> Option<&str> {
        self.lookup_hint_id.as_deref()
    }

    /// 成功审核的必检覆盖可交给应用继续作发布决定。
    pub fn decision(&self) -> Option<Decision> {
        let result = self.assessment()?;
        if self.state != ReviewState::Succeeded
            || self.profile.required_modalities().is_empty()
            || self
                .profile
                .required_modalities()
                .iter()
                .any(|&kind| result.coverage(kind) != Coverage::Complete)
        {
            return None;
        }
        Some(result.recommendation())
    }

    pub(crate) fn matches(&self, request: &VideoReviewRequest) -> bool {
        self.content_id == request.content_id
            && self.content_revision == request.content_revision
            && self.source == request.source
            && self.profile == request.profile
            && self.deadline_at.checked_sub(self.created_at)
                == Some(i64::from(request.timeout_seconds))
    }
}
