//! 具名内容审核驱动以及应用和供应商之间的审核契约。

#![forbid(unsafe_code)]

#[cfg(feature = "webhooks")]
mod aliyun_callback;
#[path = "../config/moderation.rs"]
mod configuration;
mod contracts;
mod credentials;
mod drivers;
#[cfg(feature = "webhooks")]
mod hive_lookup_hint;
mod input;
mod late_notice;
mod manager;
mod moderation;
mod provider;
#[cfg(feature = "webhooks")]
mod provider_webhooks;
mod query_policy;
mod rate_policy;
mod recovery;
mod result;
mod review;
mod review_decision;
mod review_group;
mod revision;
#[cfg(feature = "webhooks")]
mod sightengine_callback;
mod storage;
#[cfg(feature = "webhooks")]
mod tencent_callback;
#[cfg(feature = "webhooks")]
mod video_callback;
mod video_service;

#[cfg(feature = "webhooks")]
pub use aliyun_callback::AliyunCallbackProcessor;
pub use contracts::{
    ModerationDriver, ModerationError, ModerationFuture, ModerationResult, Operation,
};
pub use credentials::{CredentialReference, ModerationCredentialProvider, ModerationCredentials};
#[cfg(feature = "webhooks")]
pub use hive_lookup_hint::HiveLookupHint;
pub use input::{ImageInput, StoredMedia, TaskId, TextInput, VideoInput};
pub use late_notice::{LateVideoNotice, LateVideoNoticeKind};
pub use manager::ModerationManager;
pub use moderation::Moderation;
pub use openrust::http::client::RetryAfter;
pub use provider::{ModerationServiceProvider, openrust_providers};
pub use result::{
    Assessment, Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, TaskStatus,
    VideoInspection,
};
pub use review::{
    ProviderCancellation, QueryFailure, ReviewState, SubmissionFailure, VideoReview,
    VideoReviewRequest, VideoSubmission,
};
pub use review_decision::{
    ManualReviewDecision, ReviewDecision, ReviewDecisionRequest, VideoReviewTarget,
};
pub use review_group::{VideoReviewGroup, VideoReviewGroupRequest};
pub use revision::ProfileRevision;
#[cfg(feature = "webhooks")]
pub use sightengine_callback::SightengineCallbackProcessor;
pub use storage::ModerationStore;
#[cfg(feature = "webhooks")]
pub use tencent_callback::TencentCallbackProcessor;
pub use video_service::VideoReviewService;
