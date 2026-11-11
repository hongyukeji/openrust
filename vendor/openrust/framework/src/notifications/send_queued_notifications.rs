use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    contracts::queue::JobHandler,
    database::Transaction,
    queue::{DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueResult},
    support::facades::{Facade, Queue},
};

use super::{
    NotificationError, NotificationFacade, NotificationResult, notification_sender::Delivery,
};

/// The worker handler for one queued notification channel delivery.
pub struct SendQueuedNotifications;

#[derive(Serialize, Deserialize)]
struct Job<T> {
    delivery: T,
    payload: Value,
}

impl SendQueuedNotifications {
    pub(crate) const NAME: &str = "notifications.send_queued_notifications";

    pub(super) async fn dispatch(
        delivery: &Delivery,
        payload: Value,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> NotificationResult<()> {
        let payload = Payload::new(Self::NAME, Job { delivery, payload })
            .map_err(NotificationError::Queue)?;
        Queue::dispatch(payload, options, transaction)
            .await
            .map_err(NotificationError::Queue)?;
        Ok(())
    }
}

impl JobHandler for SendQueuedNotifications {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let Job::<Delivery> { delivery, payload } =
                serde_json::from_value(payload.data().clone())
                    .map_err(|_| QueueError::InvalidPayload)?;
            let result = async {
                let manager = NotificationFacade::getFacadeRoot().map_err(super::delivery)?;
                let channel = manager.channel(Some(&delivery.channel))?;
                delivery
                    .localized(delivery.send(channel.send_queued(payload)))
                    .await
            }
            .await;
            result.map_err(|source| QueueError::Execution {
                source: Box::new(source),
            })
        })
    }
}
