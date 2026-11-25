use std::sync::Arc;

use crate::{
    ModerationDriver, ModerationError, ModerationManager, ModerationResult, ModerationStore,
    Operation, SubmissionFailure, VideoInput, VideoReview, VideoSubmission,
};

/// 点播送审流程，使用已落库的对象版本、配置修订和租约。
pub struct VideoReviewService {
    store: ModerationStore,
    manager: Arc<ModerationManager>,
}

impl VideoReviewService {
    pub fn new(store: ModerationStore, manager: Arc<ModerationManager>) -> Self {
        Self { store, manager }
    }

    /// 领取待提交检查。提交结果不明确或进程中断时保留未知状态供补偿。
    pub async fn submit(&self, id: &str) -> ModerationResult<VideoReview> {
        let review = self.find(id).await?;
        if let Some(claim) = self.store.claim_submission(&review).await? {
            let outcome = match self.driver(&claim) {
                Ok(driver) if self.store.submission_is_live(&claim).await? => {
                    Self::send(driver.as_ref(), claim.source().clone()).await
                }
                Ok(_) => {
                    self.store.recover_submission(&claim).await?;
                    return self.find(id).await;
                }
                Err(failure) => VideoSubmission::Failed(failure),
            };
            self.store.finish_submission(&claim, outcome).await?;
        } else {
            self.store.recover_submission(&review).await?;
        }
        self.find(id).await
    }

    async fn find(&self, id: &str) -> ModerationResult<VideoReview> {
        self.store
            .find(id)
            .await?
            .ok_or(ModerationError::ReviewNotFound)
    }

    fn driver(&self, claim: &VideoReview) -> Result<Arc<dyn ModerationDriver>, SubmissionFailure> {
        let driver = match self.manager.driver_for_revision(claim.profile()) {
            Ok(driver) => driver,
            Err(ModerationError::ProfileRevisionUnavailable) => {
                return Err(SubmissionFailure::RevisionUnavailable);
            }
            Err(ModerationError::ProfileRevisionChanged) => {
                return Err(SubmissionFailure::RevisionChanged);
            }
            Err(_) => return Err(SubmissionFailure::Configuration),
        };
        if !driver.supports(Operation::SubmitVideo) {
            return Err(SubmissionFailure::Unsupported);
        }
        Ok(driver)
    }

    async fn send(driver: &dyn ModerationDriver, source: crate::StoredMedia) -> VideoSubmission {
        let source = VideoInput::new(source);
        match driver.submit_video(&source).await {
            Ok(task) => VideoSubmission::Submitted(task),
            Err(ModerationError::InvalidConfiguration(_)) => {
                VideoSubmission::Failed(SubmissionFailure::Configuration)
            }
            Err(ModerationError::InvalidInput(_)) => {
                VideoSubmission::Failed(SubmissionFailure::Input)
            }
            Err(ModerationError::Unsupported(_)) => {
                VideoSubmission::Failed(SubmissionFailure::Unsupported)
            }
            Err(ModerationError::MediaUnavailable) => {
                VideoSubmission::Failed(SubmissionFailure::Media)
            }
            Err(ModerationError::Unauthorized) => {
                VideoSubmission::Failed(SubmissionFailure::Unauthorized)
            }
            Err(ModerationError::Rejected) => VideoSubmission::Failed(SubmissionFailure::Rejected),
            Err(ModerationError::Throttled) => {
                VideoSubmission::Failed(SubmissionFailure::Throttled)
            }
            Err(_) => VideoSubmission::Unknown,
        }
    }
}
