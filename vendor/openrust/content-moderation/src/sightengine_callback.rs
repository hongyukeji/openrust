use openrust::{http::HttpFuture, queue::JobContext};
use openrust_webhook_client::{
    Error, Result, WebhookStore,
    jobs::{ProcessWebhookJob, WebhookProcessingOutcome},
    models::WebhookCall,
};
use serde::Deserialize;

use crate::{
    ModerationError, ModerationResult, ModerationStore, ReviewState, TaskId,
    credentials::valid_identifier, drivers::sightengine_media_id, storage::ProviderTask,
};

/// 将已经过 Webhook Client 验签的账户通知关联到原审核。
pub struct SightengineCallbackProcessor {
    store: ModerationStore,
    account: String,
    route_profile: String,
    route_revision: String,
    webhook_scope: String,
}

impl SightengineCallbackProcessor {
    pub fn new(
        store: ModerationStore,
        account: &str,
        webhook_scope: &str,
        route_profile: &str,
        route_revision: &str,
    ) -> ModerationResult<Self> {
        if ![account, route_profile, route_revision]
            .into_iter()
            .all(valid_identifier)
            || webhook_scope.is_empty()
            || webhook_scope.len() > 128
            || webhook_scope.chars().any(char::is_control)
        {
            return Err(ModerationError::InvalidConfiguration(
                "sightengine callback scope",
            ));
        }
        Ok(Self {
            store,
            account: account.to_owned(),
            route_profile: route_profile.to_owned(),
            route_revision: route_revision.to_owned(),
            webhook_scope: webhook_scope.to_owned(),
        })
    }

    async fn process(&self, call: &WebhookCall) -> Result<WebhookProcessingOutcome> {
        use WebhookProcessingOutcome::{Completed, Deferred, PermanentFailure};
        if call.scope() != self.webhook_scope
            || call.name() != self.route_profile
            || call.revision() != self.route_revision
        {
            return Ok(PermanentFailure);
        }
        let Ok(callback) = serde_json::from_slice::<Callback>(call.raw_body()) else {
            return Ok(PermanentFailure);
        };
        if !sightengine_media_id(&callback.media.id) {
            return Ok(PermanentFailure);
        }
        if callback.data.status == Status::Ongoing {
            return Ok(Completed);
        }
        let task = TaskId::new(callback.media.id).map_err(|_| Error::InvalidRecord)?;
        let review = match self
            .store
            .find_provider_task("sightengine", &self.account, &task)
            .await
            .map_err(storage_error)?
        {
            ProviderTask::Missing => return Ok(Deferred { delay_seconds: 10 }),
            ProviderTask::Ambiguous => return Ok(PermanentFailure),
            ProviderTask::Found(review) => review,
        };
        if review.state() != ReviewState::Running
            || review.confirmation_callback_id().is_some()
            || self
                .store
                .request_confirmation(&review, call.id())
                .await
                .map_err(storage_error)?
        {
            return Ok(Completed);
        }
        // 领取或终态写入可能在匹配后先胜出，再读取一次后决定是否延期。
        let current = self.store.find(review.id()).await.map_err(storage_error)?;
        if current.is_none_or(|review| {
            review.state() != ReviewState::Running || review.confirmation_callback_id().is_some()
        }) {
            Ok(Completed)
        } else {
            Ok(Deferred { delay_seconds: 10 })
        }
    }
}

impl ProcessWebhookJob for SightengineCallbackProcessor {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        call: &'a WebhookCall,
        _: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            match self.process(call).await? {
                WebhookProcessingOutcome::Completed => Ok(()),
                _ => Err(Error::InvalidConfig(
                    "sightengine callback requires deferred processing",
                )),
            }
        })
    }

    fn handle_outcome<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        call: &'a WebhookCall,
        _: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, Result<WebhookProcessingOutcome>> {
        Box::pin(self.process(call))
    }
}

#[derive(Deserialize)]
struct Callback {
    media: Media,
    data: Data,
}

#[derive(Deserialize)]
struct Media {
    id: String,
}

#[derive(Deserialize)]
struct Data {
    status: Status,
}

#[derive(Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ongoing,
    Finished,
    Stopped,
    Failure,
}

fn storage_error(source: ModerationError) -> Error {
    Error::Runtime {
        source: Box::new(source),
    }
}
