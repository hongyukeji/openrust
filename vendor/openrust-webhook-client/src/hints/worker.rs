use super::{ProcessWebhookHint, storage::HintStore, valid_identifier, wire::Receipt};
use crate::{Error, jobs::queue_error};
use openrust::{
    contracts::queue::JobHandler,
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult},
};
use serde::Deserialize;
use std::sync::Arc;

pub(super) struct HintJob {
    store: HintStore,
    handler: Arc<dyn ProcessWebhookHint>,
}

impl HintJob {
    pub(super) fn new(store: HintStore, handler: Arc<dyn ProcessWebhookHint>) -> Self {
        Self { store, handler }
    }
}

impl JobHandler for HintJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let receipt =
                Receipt::deserialize(payload.data()).map_err(|_| QueueError::InvalidPayload)?;
            if receipt.id.len() != 36 || !valid_identifier(receipt.revision) {
                return Err(QueueError::InvalidPayload);
            }
            let Some(hint) = self
                .store
                .claim(receipt.id, receipt.revision)
                .await
                .map_err(queue_error)?
            else {
                return Ok(());
            };
            let result = async {
                if !self.handler.is_registered(&hint).await? {
                    return Ok("ignored");
                }
                self.handler.handle(&hint).await?;
                Ok::<_, Error>("completed")
            }
            .await;
            let status = result.unwrap_or("failed");
            self.store
                .finish(&hint, status)
                .await
                .map_err(queue_error)?;
            if status == "failed" {
                return Err(queue_error(Error::HintProcessingFailed));
            }
            Ok(())
        })
    }
}
