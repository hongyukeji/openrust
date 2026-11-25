//! 具名内容审核驱动以及应用和供应商之间的审核契约。

#![forbid(unsafe_code)]

#[path = "../config/moderation.rs"]
mod configuration;
mod contracts;
mod credentials;
mod drivers;
mod input;
mod manager;
mod moderation;
mod provider;
#[cfg(feature = "webhooks")]
mod provider_webhooks;
mod query_policy;
mod recovery;
mod result;
mod review;
mod revision;
mod storage;
mod video_service;

pub use contracts::{
    ModerationDriver, ModerationError, ModerationFuture, ModerationResult, Operation,
};
pub use credentials::{CredentialReference, ModerationCredentialProvider, ModerationCredentials};
pub use input::{ImageInput, StoredMedia, TaskId, TextInput, VideoInput};
pub use manager::ModerationManager;
pub use moderation::Moderation;
pub use provider::{ModerationServiceProvider, openrust_providers};
pub use result::{
    Assessment, Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, TaskStatus,
};
pub use review::{
    QueryFailure, ReviewState, SubmissionFailure, VideoReview, VideoReviewRequest, VideoSubmission,
};
pub use revision::ProfileRevision;
pub use storage::ModerationStore;
pub use video_service::VideoReviewService;
