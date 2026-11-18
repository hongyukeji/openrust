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
    support::facades::{Crypt, Facade, Queue},
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

#[derive(Deserialize)]
#[serde(untagged)]
enum QueuedDelivery {
    Encrypted { encrypted_delivery: String },
    Plain(Box<Job<Delivery>>),
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

    #[allow(
        clippy::too_many_arguments,
        reason = "Extends the queued notification dispatch contract"
    )]
    pub(super) async fn dispatch(
        delivery: &Delivery,
        payload: Value,
        options: DispatchOptions,
        job_options: JobOptions,
        middleware: Option<&str>,
        encrypt: bool,
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
        let job = Job { delivery, payload };
        let payload = if encrypt {
            let json = serde_json::to_string(&job).map_err(super::delivery)?;
            let encrypted_delivery = Crypt::encryptString(&json)
                .map_err(|_| super::delivery("Could not encrypt the queued notification."))?;
            Payload::new(
                name.as_deref().unwrap_or(Self::NAME),
                serde_json::json!({ "encrypted_delivery": encrypted_delivery }),
            )
        } else {
            Payload::new(name.as_deref().unwrap_or(Self::NAME), job)
        }
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
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let job = match serde_json::from_value(payload.data().clone()) {
                Ok(QueuedDelivery::Plain(job)) => Ok(*job),
                Ok(QueuedDelivery::Encrypted { encrypted_delivery }) => {
                    Crypt::decryptString(&encrypted_delivery)
                        .map_err(|_| QueueError::InvalidPayload)
                        .and_then(|json| {
                            serde_json::from_slice(&json).map_err(|_| QueueError::InvalidPayload)
                        })
                }
                Err(_) => Err(QueueError::InvalidPayload),
            };
            let Job { delivery, payload } = match job {
                Ok(job) => job,
                Err(error) => {
                    context.fail_without_retry();
                    return Err(error);
                }
            };
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
                        let result = delivery.send(channel.send_queued(payload)).await;
                        if channel.is_sms()
                            && let Err(error) = &result
                            && !super::sms::SmsFailure::retryable(error)
                        {
                            context.fail_without_retry();
                        }
                        result
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
