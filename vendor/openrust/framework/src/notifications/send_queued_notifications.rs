use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    contracts::queue::JobHandler,
    database::Transaction,
    queue::{
        DispatchOptions, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueManager,
        QueueResult, middleware::JobMiddleware,
    },
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

    /// Registers a middleware chain for notifications selecting this name.
    #[allow(non_snake_case, reason = "Uses Laravel's queue API naming")]
    pub fn registerMiddleware(
        queue: &QueueManager,
        name: &str,
        middleware: Vec<Arc<dyn JobMiddleware>>,
    ) -> QueueResult<()> {
        queue.register(
            &Self::middleware_job(name)?,
            Arc::new(MiddlewareDelivery(middleware)),
        )
    }

    fn middleware_job(name: &str) -> QueueResult<String> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(format!("{}.{name}", Self::NAME))
    }

    pub(super) async fn dispatch(
        delivery: &Delivery,
        payload: Value,
        options: DispatchOptions,
        job_options: JobOptions,
        middleware: Option<&str>,
        transaction: Option<&mut Transaction>,
    ) -> NotificationResult<()> {
        let name = middleware
            .map(Self::middleware_job)
            .transpose()
            .map_err(NotificationError::Queue)?;
        if let Some(name) = &name {
            let queue = Queue::getFacadeRoot().map_err(super::delivery)?;
            if queue.command_handler(name).is_none() {
                return Err(NotificationError::Queue(QueueError::UnknownJob(
                    name.clone(),
                )));
            }
        }
        let payload = Payload::new(
            name.as_deref().unwrap_or(Self::NAME),
            Job { delivery, payload },
        )
        .map_err(NotificationError::Queue)?
        .with_options(job_options);
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
                delivery
                    .localized(async {
                        if let Some(guard) = &delivery.guard
                            && !manager.allows_queued(guard).await?
                        {
                            return delivery.skipped().await;
                        }
                        let channel = manager.channel(Some(&delivery.channel))?;
                        delivery.send(channel.send_queued(payload)).await
                    })
                    .await
            }
            .await;
            result.map_err(|source| QueueError::Execution {
                source: Box::new(source),
            })
        })
    }
}

struct MiddlewareDelivery(Vec<Arc<dyn JobMiddleware>>);

impl JobHandler for MiddlewareDelivery {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        self.0.clone()
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        SendQueuedNotifications.handle(context, payload)
    }
}
