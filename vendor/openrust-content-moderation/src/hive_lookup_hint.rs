use openrust::http::HttpFuture;
use openrust_webhook_client::{
    Error, Result,
    hints::{ProcessWebhookHint, UnverifiedHint, WebhookHint, WebhookHintPolicy},
};
use serde::Deserialize;

use crate::{
    ModerationError, ModerationManager, ModerationResult, ModerationStore, ProfileRevision,
    ReviewState, TaskId, VideoReview, VideoSubmission, credentials::valid_identifier,
    drivers::valid_hive_task, storage::ProviderTask,
};

/// Hive 通知只为已确认的任务请求提前查单，Worker 以原项目凭据查询。
pub struct HiveLookupHint {
    store: ModerationStore,
    profile: ProfileRevision,
    project: u64,
    webhook_scope: String,
    route_profile: String,
    route_revision: String,
}

impl HiveLookupHint {
    pub fn new(
        store: ModerationStore,
        manager: &ModerationManager,
        profile: ProfileRevision,
        webhook_scope: &str,
        route_profile: &str,
        route_revision: &str,
    ) -> ModerationResult<Self> {
        let configuration = manager.config_for_revision(&profile)?;
        if profile.driver() != "hive"
            || configuration["video_query_api"].as_str() != Some("v1-task-summary")
        {
            return Err(ModerationError::InvalidConfiguration("Hive lookup profile"));
        }
        let project = configuration["video_project"]
            .as_u64()
            .filter(|&project| project > 0)
            .ok_or(ModerationError::InvalidConfiguration("video_project"))?;
        if ![webhook_scope, route_profile, route_revision]
            .into_iter()
            .all(valid_identifier)
        {
            return Err(ModerationError::InvalidConfiguration(
                "Hive hint route scope",
            ));
        }
        Ok(Self {
            store,
            profile,
            project,
            webhook_scope: webhook_scope.to_owned(),
            route_profile: route_profile.to_owned(),
            route_revision: route_revision.to_owned(),
        })
    }

    fn eligible(&self, review: &VideoReview) -> bool {
        review.profile() == &self.profile
            && review.state() == ReviewState::Running
            && matches!(review.submission(), VideoSubmission::Submitted(_))
            && review.lookup_hint_id().is_none()
    }

    async fn registered(&self, hint: &WebhookHint) -> Result<Option<VideoReview>> {
        if hint.scope() != self.webhook_scope
            || hint.profile() != self.route_profile
            || hint.revision() != self.route_revision
        {
            return Ok(None);
        }
        self.store
            .find(hint.task_id())
            .await
            .map(|review| review.filter(|review| self.eligible(review)))
            .map_err(storage_error)
    }
}

impl WebhookHintPolicy for HiveLookupHint {
    fn resolve<'a>(
        &'a self,
        scope: &'a str,
        hint: UnverifiedHint<'a>,
    ) -> HttpFuture<'a, Result<Option<String>>> {
        Box::pin(async move {
            if scope != self.webhook_scope
                || hint.profile() != self.route_profile
                || hint.revision() != self.route_revision
            {
                return Ok(None);
            }
            let body = hint.request().body();
            if body.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
                return Ok(None);
            }
            let Ok(notification) = serde_json::from_slice::<Notification>(body) else {
                return Ok(None);
            };
            if notification.project_id != self.project || !valid_hive_task(&notification.id) {
                return Ok(None);
            }
            let task = TaskId::new(notification.id).map_err(storage_error)?;
            if self
                .store
                .has_video_tombstone("hive", self.profile.account(), &task)
                .await
                .map_err(storage_error)?
            {
                return Ok(None);
            }
            let result = self
                .store
                .find_provider_task("hive", self.profile.account(), &task)
                .await
                .map_err(storage_error)?;
            match result {
                ProviderTask::Found(review) if self.eligible(&review) => {
                    Ok(Some(review.id().to_owned()))
                }
                _ => Ok(None),
            }
        })
    }
}

impl ProcessWebhookHint for HiveLookupHint {
    fn is_registered<'a>(&'a self, hint: &'a WebhookHint) -> HttpFuture<'a, Result<bool>> {
        Box::pin(async move { Ok(self.registered(hint).await?.is_some()) })
    }

    fn handle<'a>(&'a self, hint: &'a WebhookHint) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            if let Some(review) = self.registered(hint).await? {
                self.store
                    .request_lookup_hint(&review, hint.id())
                    .await
                    .map_err(storage_error)?;
            }
            Ok(())
        })
    }
}

#[derive(Deserialize)]
struct Notification {
    id: String,
    project_id: u64,
}

fn storage_error(source: ModerationError) -> Error {
    Error::Runtime {
        source: Box::new(source),
    }
}
