use std::sync::Arc;

use crate::{
    Coverage, ModerationDriver, ModerationError, ModerationManager, ModerationResult,
    ModerationStore, Operation, ProviderCancellation, QueryFailure, ReviewState, SubmissionFailure,
    TaskId, TaskStatus, VideoInput, VideoReview, VideoReviewGroup, VideoSubmission,
    review::QueryOutcome, storage::Cancellation,
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
        self.submit_review(&review).await?;
        self.find(id).await
    }

    async fn submit_review(&self, review: &VideoReview) -> ModerationResult<()> {
        if let Some(claim) = self.store.claim_submission(review).await? {
            let outcome = match self.driver(&claim) {
                Ok(driver) if self.store.submission_is_live(&claim).await? => {
                    Self::send(driver.as_ref(), claim.source().clone()).await
                }
                Ok(_) => {
                    self.store.recover_submission(&claim).await?;
                    return Ok(());
                }
                Err(failure) => VideoSubmission::Failed(failure),
            };
            self.store.finish_submission(&claim, outcome).await?;
        } else {
            self.store.recover_submission(review).await?;
        }
        Ok(())
    }

    /// 对已确认的供应商任务执行有限预算内的一次查单，或返回目前已持久化的状态。
    pub async fn query(&self, id: &str) -> ModerationResult<VideoReview> {
        let review = self.find(id).await?;
        self.query_review(&review).await?;
        self.find(id).await
    }

    /// 先冻结本地审核，再按原配置尝试取消已确认的供应商任务。
    pub async fn cancel(&self, id: &str) -> ModerationResult<VideoReview> {
        for _ in 0..4 {
            let review = self.find(id).await?;
            if !matches!(review.state(), ReviewState::Pending | ReviewState::Running) {
                return Ok(review);
            }
            if let Some(cancellation) = self.store.cancel_active(&review).await? {
                self.finish_cancelled(&review, cancellation).await?;
                return self.find(id).await;
            }
        }
        Err(ModerationError::ReviewConflict)
    }

    /// 同一事务冻结组内未结束的检查。已结束的检查仍保留证据。
    pub async fn cancel_group(&self, id: &str) -> ModerationResult<VideoReviewGroup> {
        for _ in 0..4 {
            let group = self.find_group(id).await?;
            if !matches!(group.state(), ReviewState::Pending | ReviewState::Running) {
                return Ok(group);
            }
            if let Some(frozen) = self.store.cancel_group_active(&group).await? {
                for (review, cancellation) in frozen {
                    self.finish_cancelled(&review, cancellation).await?;
                }
                return self.find_group(id).await;
            }
        }
        Err(ModerationError::ReviewConflict)
    }

    async fn find_group(&self, id: &str) -> ModerationResult<VideoReviewGroup> {
        self.store
            .find_video_group(id)
            .await?
            .ok_or(ModerationError::ReviewNotFound)
    }

    async fn finish_cancelled(
        &self,
        review: &VideoReview,
        cancellation: Cancellation,
    ) -> ModerationResult<()> {
        if let Cancellation::Provider { version, task } = cancellation {
            let result = self.cancel_provider(review, &task).await;
            self.store
                .finish_cancellation(review, version, result)
                .await?;
        }
        Ok(())
    }

    async fn cancel_provider(&self, review: &VideoReview, task: &TaskId) -> ProviderCancellation {
        let Ok(driver) = self.manager.driver_for_revision(review.profile()) else {
            return ProviderCancellation::Unavailable;
        };
        if !driver.supports(Operation::CancelVideo) {
            return ProviderCancellation::Unsupported;
        }
        match driver.cancel_video(task).await {
            Ok(()) => ProviderCancellation::Acknowledged,
            Err(ModerationError::Rejected | ModerationError::Unauthorized) => {
                ProviderCancellation::Rejected
            }
            Err(ModerationError::Unsupported(_)) => ProviderCancellation::Unsupported,
            Err(_) => ProviderCancellation::Unconfirmed,
        }
    }

    pub(crate) async fn advance(&self, review: &VideoReview) -> ModerationResult<()> {
        if matches!(review.submission(), VideoSubmission::Submitted(_)) {
            self.query_review(review).await
        } else {
            self.submit_review(review).await
        }
    }

    async fn query_review(&self, review: &VideoReview) -> ModerationResult<()> {
        let Some(claim) = self.store.claim_query(review).await? else {
            return Ok(());
        };
        let VideoSubmission::Submitted(task) = claim.submission() else {
            return Err(ModerationError::ReviewStorage);
        };
        let outcome = match self.manager.driver_for_revision(claim.profile()) {
            Ok(driver) if driver.supports(Operation::QueryVideo) => {
                if !self.store.query_is_live(&claim).await? {
                    return Ok(());
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
        Ok(())
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
