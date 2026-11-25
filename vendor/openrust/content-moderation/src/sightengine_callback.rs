use openrust::{http::HttpFuture, queue::JobContext};
use openrust_webhook_client::{
    Error, Result, WebhookStore,
    jobs::{ProcessWebhookJob, WebhookProcessingOutcome},
    models::WebhookCall,
};
use serde::Deserialize;

use crate::{
    ModerationResult, ModerationStore, TaskId, drivers::sightengine_media_id,
    video_callback::CallbackConfirmation,
};

/// 将已经过 Webhook Client 验签的账户通知关联到原审核。
pub struct SightengineCallbackProcessor {
    confirmation: CallbackConfirmation,
}

impl SightengineCallbackProcessor {
    pub fn new(
        store: ModerationStore,
        account: &str,
        webhook_scope: &str,
        route_profile: &str,
        route_revision: &str,
    ) -> ModerationResult<Self> {
        Ok(Self {
            confirmation: CallbackConfirmation::new(
                store,
                "sightengine",
                account,
                webhook_scope,
                route_profile,
                route_revision,
            )?,
        })
    }

    async fn process(&self, call: &WebhookCall) -> Result<WebhookProcessingOutcome> {
        use WebhookProcessingOutcome::{Completed, PermanentFailure};
        if !self.confirmation.accepts(call) {
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
        self.confirmation.confirm(call.id(), task).await
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
