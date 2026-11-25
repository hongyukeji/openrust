use crate::{ModerationError, ModerationResult, ReviewState};

/// 已被截止、取消或提交恢复隔离的供应商结果来源。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LateVideoNoticeKind {
    Submission,
    Lookup,
    SignedCallback,
}

impl LateVideoNoticeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submission => "submission",
            Self::Lookup => "lookup",
            Self::SignedCallback => "signed_callback",
        }
    }

    pub(crate) fn parse(value: &str) -> ModerationResult<Self> {
        match value {
            "submission" => Ok(Self::Submission),
            "lookup" => Ok(Self::Lookup),
            "signed_callback" => Ok(Self::SignedCallback),
            _ => Err(ModerationError::ReviewStorage),
        }
    }
}

/// 与审核结论分开保存的有限诊断。
pub struct LateVideoNotice {
    pub kind: LateVideoNoticeKind,
    pub state: ReviewState,
    pub observed_at: i64,
    pub receipt_id: Option<String>,
}
