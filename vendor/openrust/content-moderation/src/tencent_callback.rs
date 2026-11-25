use openrust::{http::HttpFuture, queue::JobContext};
use openrust_webhook_client::{
    Error, Result, WebhookStore,
    jobs::{ProcessWebhookJob, WebhookProcessingOutcome},
    models::WebhookCall,
};
use serde::Deserialize;

use crate::{
    ModerationError, ModerationResult, ModerationStore, TaskId,
    drivers::valid_tencent_video_policy, video_callback::CallbackConfirmation,
};

/// 腾讯云标准 VM 点播通知只请求以原任务凭据查单。
pub struct TencentCallbackProcessor {
    confirmation: CallbackConfirmation,
    policy: String,
}

impl TencentCallbackProcessor {
    pub fn new(
        store: ModerationStore,
        account: &str,
        policy: &str,
        webhook_scope: &str,
        route_profile: &str,
        route_revision: &str,
    ) -> ModerationResult<Self> {
        if !valid_tencent_video_policy(policy) {
            return Err(ModerationError::InvalidConfiguration("video_biz_type"));
        }
        Ok(Self {
            confirmation: CallbackConfirmation::new(
                store,
                "tencent",
                account,
                webhook_scope,
                route_profile,
                route_revision,
            )?,
            policy: policy.to_owned(),
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
        if callback.kind != "VIDEO" || callback.biz_type != self.policy {
            return Ok(PermanentFailure);
        }
        let Ok(task) = TaskId::new(callback.task_id) else {
            return Ok(PermanentFailure);
        };
        match callback.status {
            Status::Running => Ok(Completed),
            Status::Finish | Status::Error => self.confirmation.confirm(call.id(), task).await,
        }
    }
}

impl ProcessWebhookJob for TencentCallbackProcessor {
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
                    "tencent callback requires deferred processing",
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
#[serde(rename_all = "PascalCase")]
struct Callback {
    task_id: String,
    biz_type: String,
    #[serde(rename = "Type")]
    kind: String,
    status: Status,
}

#[derive(Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum Status {
    Running,
    Finish,
    Error,
}
