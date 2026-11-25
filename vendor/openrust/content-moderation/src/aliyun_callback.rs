use std::borrow::Cow;

use openrust::{http::HttpFuture, queue::JobContext};
use openrust_webhook_client::{
    Error, Result, WebhookStore,
    jobs::{ProcessWebhookJob, WebhookProcessingOutcome},
    models::WebhookCall,
};
use serde::Deserialize;

use crate::{
    ModerationResult, ModerationStore, TaskId, drivers::AliyunVideoCode,
    video_callback::CallbackConfirmation,
};

pub(crate) struct CallbackForm<'a> {
    pub(crate) checksum: Cow<'a, str>,
    pub(crate) content: Cow<'a, str>,
}

impl<'a> CallbackForm<'a> {
    pub(crate) fn parse(body: &'a [u8]) -> Option<Self> {
        let mut checksum = None;
        let mut content = None;
        for (key, value) in url::form_urlencoded::parse(body) {
            match key.as_ref() {
                "checksum" if checksum.is_none() => checksum = Some(value),
                "content" if content.is_none() => content = Some(value),
                _ => return None,
            }
        }
        Some(Self {
            checksum: checksum?,
            content: content?,
        })
    }

    pub(crate) fn contains_checksum(body: &[u8]) -> bool {
        url::form_urlencoded::parse(body).any(|(key, _)| key == "checksum")
    }
}

/// 阿里云增强版点播通知只请求以原任务凭据查单。
pub struct AliyunCallbackProcessor {
    confirmation: CallbackConfirmation,
}

impl AliyunCallbackProcessor {
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
                "aliyun",
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
        let Some(form) = CallbackForm::parse(call.raw_body()) else {
            return Ok(PermanentFailure);
        };
        let Ok(callback) = serde_json::from_str::<Callback>(&form.content) else {
            return Ok(PermanentFailure);
        };
        match callback.code.number() {
            Ok(280 | 288) => Ok(Completed),
            Ok(200 | 404..=407 | 409) => {
                let Some(task) = callback
                    .data
                    .and_then(|data| TaskId::new(data.task_id).ok())
                else {
                    return Ok(PermanentFailure);
                };
                self.confirmation.confirm(call.id(), task).await
            }
            _ => Ok(PermanentFailure),
        }
    }
}

impl ProcessWebhookJob for AliyunCallbackProcessor {
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
                    "aliyun callback requires deferred processing",
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
    code: AliyunVideoCode,
    data: Option<Data>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Data {
    task_id: String,
}
