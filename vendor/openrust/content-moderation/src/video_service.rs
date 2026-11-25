use std::sync::Arc;

use crate::{
    Coverage, ModerationDriver, ModerationError, ModerationManager, ModerationResult,
    ModerationStore, Operation, QueryFailure, ReviewState, SubmissionFailure, TaskId, TaskStatus,
    VideoInput, VideoReview, VideoSubmission, review::QueryOutcome,
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

    /// 对已确认的供应商任务执行有限预算内的一次查单，或返回目前已持久化的状态。
    pub async fn query(&self, id: &str) -> ModerationResult<VideoReview> {
        let review = self.find(id).await?;
        let Some(claim) = self.store.claim_query(&review).await? else {
            return self.find(id).await;
        };
        let VideoSubmission::Submitted(task) = claim.submission() else {
            return Err(ModerationError::ReviewStorage);
        };
        let outcome = match self.manager.driver_for_revision(claim.profile()) {
            Ok(driver) if driver.supports(Operation::QueryVideo) => {
                if !self.store.query_is_live(&claim).await? {
                    return self.find(id).await;
                }
                Self::read_provider(driver.as_ref(), task, &claim).await
            }
            Ok(_) => query_failed(QueryFailure::Unsupported),
            Err(ModerationError::ProfileRevisionUnavailable) => {
                query_failed(QueryFailure::RevisionUnavailable)
            }
            Err(ModerationError::ProfileRevisionChanged) => {
                query_failed(QueryFailure::RevisionChanged)
            }
            Err(_) => query_failed(QueryFailure::Configuration),
        };
        self.store.finish_query(&claim, outcome).await?;
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

    async fn read_provider(
        driver: &dyn ModerationDriver,
        task: &TaskId,
        review: &VideoReview,
    ) -> QueryOutcome {
        match driver.query_video(task).await {
            Ok(TaskStatus::Pending | TaskStatus::Running) => QueryOutcome::Waiting,
            Ok(TaskStatus::Succeeded(assessment)) => {
                let complete = review
                    .profile()
                    .required_modalities()
                    .iter()
                    .all(|&modality| assessment.coverage(modality) == Coverage::Complete);
                QueryOutcome::Stop {
                    state: if complete {
                        ReviewState::Succeeded
                    } else {
                        ReviewState::Failed
                    },
                    failure: (!complete).then_some(QueryFailure::IncompleteCoverage),
                    assessment: Some(assessment),
                }
            }
            Ok(TaskStatus::Failed) => query_failed(QueryFailure::ProviderFailed),
            Ok(TaskStatus::Cancelled) => query_terminal(ReviewState::Cancelled),
            Ok(TaskStatus::Expired) => query_terminal(ReviewState::Expired),
            Err(ModerationError::Throttled) => QueryOutcome::Retry(QueryFailure::Throttled),
            Err(ModerationError::Timeout) => QueryOutcome::Retry(QueryFailure::Timeout),
            Err(ModerationError::ServiceUnavailable) => {
                QueryOutcome::Retry(QueryFailure::ServiceUnavailable)
            }
            Err(ModerationError::Unauthorized) => query_failed(QueryFailure::Unauthorized),
            Err(ModerationError::Rejected) => query_failed(QueryFailure::Rejected),
            Err(ModerationError::Unsupported(_)) => query_failed(QueryFailure::Unsupported),
            Err(ModerationError::InvalidConfiguration(_)) => {
                query_failed(QueryFailure::Configuration)
            }
            Err(_) => query_failed(QueryFailure::Protocol),
        }
    }
}

fn query_terminal(state: ReviewState) -> QueryOutcome {
    QueryOutcome::Stop {
        state,
        failure: None,
        assessment: None,
    }
}

fn query_failed(failure: QueryFailure) -> QueryOutcome {
    QueryOutcome::Stop {
        state: ReviewState::Failed,
        failure: Some(failure),
        assessment: None,
    }
}
