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
    NotificationError, NotificationFacade, NotificationResult,
    channels::Channel,
    notification_sender::Delivery,
    sms_delivery::{SmsDelivery, Status},
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
    pub fn register_middleware(
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
        subject_type: &str,
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
            let queue = Queue::get_facade_root().map_err(super::delivery)?;
            if queue.command_handler(name).is_none() {
                return Err(NotificationError::Queue(QueueError::UnknownJob(
                    name.clone(),
                )));
            }
        }
        let job = Job { delivery, payload };
        let payload = if encrypt {
            let json = serde_json::to_string(&job).map_err(super::delivery)?;
            let encrypted_delivery = Crypt::encrypt_string(&json)
                .map_err(|_| super::delivery("Could not encrypt the queued notification."))?;
            Payload::new(
                name.as_deref().unwrap_or(Self::NAME),
                serde_json::json!({ "encrypted_delivery": encrypted_delivery }),
            )
        } else {
            Payload::new(name.as_deref().unwrap_or(Self::NAME), job)
        }
        .map_err(NotificationError::Queue)?
        .with_job_subject(crate::queue::JobType::Notification, subject_type)
        .with_options(job_options);
        let dispatch = Queue::dispatch(payload, options, transaction);
        if encrypt {
            // 复用原生Context脱水：隐藏标记随作业恢复，明文和凭据仍不进入元数据。
            crate::log::context::Context::scope(
                Box::pin(dispatch),
                Default::default(),
                [(
                    crate::log::context::SENSITIVE_DATA.to_owned(),
                    Value::Bool(true),
                )]
                .into_iter()
                .collect(),
            )
            .await
            .map_err(super::delivery)?
            .map_err(NotificationError::Queue)?;
        } else {
            dispatch.await.map_err(NotificationError::Queue)?;
        }
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
            let job_id = payload.uuid();
            let (job, sensitive) = match serde_json::from_value(payload.data().clone()) {
                Ok(QueuedDelivery::Plain(job)) => (Ok(*job), false),
                Ok(QueuedDelivery::Encrypted { encrypted_delivery }) => (
                    Crypt::decrypt_string(&encrypted_delivery)
                        .map_err(|_| QueueError::InvalidPayload)
                        .and_then(|json| {
                            serde_json::from_slice(&json).map_err(|_| QueueError::InvalidPayload)
                        }),
                    true,
                ),
                Err(_) => (Err(QueueError::InvalidPayload), false),
            };
            let Job { delivery, payload } = match job {
                Ok(job) => job,
                Err(error) => {
                    context.fail_without_retry();
                    return Err(error);
                }
            };
            if sensitive {
                // 已入队的旧载荷也保护整个原生作业作用域；失败和中断后的诊断仍须脱敏。
                crate::log::context::Context::add_hidden(
                    crate::log::context::SENSITIVE_DATA,
                    Value::Bool(true),
                )
                .map_err(|source| QueueError::Execution {
                    source: Arc::new(source),
                })?;
            }
            let operation = Box::pin(async {
                let manager = NotificationFacade::get_facade_root().map_err(super::delivery)?;
                delivery
                    .localized(async {
                        if let Some(guard) = &delivery.guard
                            && !manager.allows_queued(guard).await?
                        {
                            return delivery.skipped().await;
                        }
                        let channel = manager.channel(Some(&delivery.channel))?;
                        if channel.is_sms() {
                            Box::pin(send_sms(
                                context,
                                job_id,
                                &delivery,
                                channel.as_ref(),
                                payload,
                            ))
                            .await
                        } else {
                            delivery.send(channel.send_queued(payload)).await
                        }
                    })
                    .await
            });
            let result = operation.await;
            result.map_err(|source| QueueError::Execution {
                source: std::sync::Arc::new(source),
            })
        })
    }
}

async fn send_sms(
    context: &mut JobContext<'_>,
    job_id: &str,
    delivery: &Delivery,
    channel: &dyn Channel,
    payload: Value,
) -> NotificationResult<()> {
    let entry = SmsDelivery::new(job_id)?;
    let status = entry.status().await?;
    match status {
        Some(Status::Accepted) => return Ok(()),
        Some(Status::Submitting | Status::Unknown | Status::Rejected) => {
            return stop_sms(context, status);
        }
        None | Some(Status::Retryable) => {}
    }

    if !delivery.can_send().await? {
        return delivery.skipped().await;
    }
    if !entry.claim(status).await? {
        return stop_sms(context, entry.status().await?);
    }
    let result = channel.send_queued(payload).await;
    let status = entry.finish(&result).await?;
    if matches!(status, Status::Rejected | Status::Unknown) {
        context.fail_without_retry();
    }
    delivery.complete(result).await
}

fn stop_sms(context: &mut JobContext<'_>, status: Option<Status>) -> NotificationResult<()> {
    if status == Some(Status::Accepted) {
        return Ok(());
    }
    context.fail_without_retry();
    let message = if status == Some(Status::Rejected) {
        "This SMS job was already rejected by the provider."
    } else {
        "This SMS job has a previous attempt whose submission must be reconciled."
    };
    Err(super::delivery(message))
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
