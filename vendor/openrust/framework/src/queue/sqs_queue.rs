#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use aws_sdk_sqs::{
    Client,
    config::{BehaviorVersion, Credentials, Region},
    types::{MessageSystemAttributeName, QueueAttributeName, SendMessageBatchRequestEntry},
};
use tokio::sync::OnceCell;

use crate::{
    cache::{Repository, Ttl, Value},
    contracts::{
        cache::Repository as _,
        queue::{ClearableQueue, Job, Queue as Contract, QueueMetrics},
    },
};

use super::{
    Delay, JobContext, Payload, Queue, QueueError, QueueFuture, QueueResult, backend,
    jobs::{Reservation, ReservedJob},
};

#[derive(serde::Deserialize)]
pub(super) struct Options {
    region: Option<String>,
    key: Option<String>,
    secret: Option<String>,
    token: Option<String>,
    endpoint: Option<String>,
    prefix: Option<String>,
    suffix: Option<String>,
    visibility_timeout: Option<i32>,
    #[serde(default)]
    block_for: Option<u32>,
    overflow: Option<Overflow>,
}

#[derive(serde::Deserialize)]
struct Overflow {
    #[serde(default)]
    enabled: bool,
    store: Option<String>,
    #[serde(default)]
    always: bool,
    #[serde(default = "default_delete")]
    delete_after_processing: bool,
    #[serde(default)]
    flush_on_clear: bool,
    ttl_seconds: Option<i64>,
}

fn default_delete() -> bool {
    true
}

#[derive(Clone)]
struct OverflowStore {
    cache: Arc<Repository>,
    always: bool,
    delete_after_processing: bool,
    flush_on_clear: bool,
    ttl: Option<Ttl>,
}

const PAYLOAD_LIMIT: usize = 1024 * 1024;
const POINTER_PREFIX: &str = "laravel:sqs-payloads:";

#[derive(serde::Deserialize, serde::Serialize)]
struct Pointer {
    #[serde(rename = "@pointer")]
    key: String,
}

pub(super) struct SqsQueue {
    name: String,
    queue: String,
    registry: Arc<Queue>,
    options: Arc<Options>,
    client: OnceCell<Client>,
    overflow: Option<OverflowStore>,
}

impl SqsQueue {
    pub(super) fn new(
        name: String,
        queue: String,
        registry: Arc<Queue>,
        options: Options,
        resolve_cache: impl FnOnce(Option<&str>) -> QueueResult<Arc<Repository>>,
    ) -> QueueResult<Self> {
        if options.key.is_some() != options.secret.is_some()
            || options
                .visibility_timeout
                .is_some_and(|value| !(0..=43200).contains(&value))
            || options.block_for.is_some_and(|value| value > 20)
        {
            return Err(QueueError::InvalidConfiguration);
        }
        let overflow = options
            .overflow
            .as_ref()
            .filter(|overflow| overflow.enabled)
            .map(|overflow| {
                if overflow.ttl_seconds.is_some_and(|ttl| ttl <= 0) {
                    return Err(QueueError::InvalidConfiguration);
                }
                Ok(OverflowStore {
                    cache: resolve_cache(overflow.store.as_deref())?,
                    always: overflow.always,
                    delete_after_processing: overflow.delete_after_processing,
                    flush_on_clear: overflow.flush_on_clear,
                    ttl: overflow.ttl_seconds.map(Ttl::Seconds),
                })
            })
            .transpose()?;
        let driver = Self {
            name,
            queue,
            registry,
            options: Arc::new(options),
            client: OnceCell::new(),
            overflow,
        };
        driver.url(None)?;
        Ok(driver)
    }

    async fn client(&self) -> &Client {
        self.client
            .get_or_init(|| async {
                let mut loader = aws_config::defaults(BehaviorVersion::latest());
                if let Some(region) = &self.options.region {
                    loader = loader.region(Region::new(region.clone()));
                }
                if let (Some(key), Some(secret)) = (&self.options.key, &self.options.secret) {
                    loader = loader.credentials_provider(Credentials::new(
                        key,
                        secret,
                        self.options.token.clone(),
                        None,
                        "queue",
                    ));
                }
                if let Some(endpoint) = &self.options.endpoint {
                    loader = loader.endpoint_url(endpoint);
                }
                Client::new(&loader.load().await)
            })
            .await
    }

    fn url(&self, queue: Option<&str>) -> QueueResult<String> {
        queue_url(queue.unwrap_or(&self.queue), &self.options)
    }
}

fn queue_url(queue: &str, options: &Options) -> QueueResult<String> {
    let url = if queue.starts_with("http://") || queue.starts_with("https://") {
        queue.to_owned()
    } else {
        let prefix = options
            .prefix
            .as_deref()
            .ok_or(QueueError::InvalidConfiguration)?;
        let suffix = options.suffix.as_deref().unwrap_or_default();
        let (name, extension) = queue
            .strip_suffix(".fifo")
            .map_or((queue, ""), |name| (name, ".fifo"));
        if name.is_empty()
            || !name
                .bytes()
                .chain(suffix.bytes())
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        format!("{}/{name}{suffix}{extension}", prefix.trim_end_matches('/'))
    };
    let parsed = url::Url::parse(&url).map_err(|_| QueueError::InvalidConfiguration)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || parsed.path().trim_matches('/').is_empty()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(QueueError::InvalidConfiguration);
    }
    Ok(url)
}

impl SqsQueue {
    async fn send(
        &self,
        payload: &Payload,
        queue: Option<&str>,
        delay: Option<Delay>,
    ) -> QueueResult<Option<String>> {
        let url = self.url(queue)?;
        send(
            self.client().await,
            &url,
            payload,
            delay,
            false,
            self.overflow.as_ref(),
        )
        .await
    }

    pub(super) async fn send_batch(&self, queue: &str, payloads: &[Payload]) -> QueueResult<()> {
        let url = self.url(Some(queue))?;
        let client = self.client().await;
        let mut entries = Vec::with_capacity(10);
        let mut bytes = 0;
        let mut start = 0;
        let mut confirmed = vec![false; payloads.len()];
        for (index, payload) in payloads.iter().enumerate() {
            let entry = prepare_message(&url, payload, None, true, self.overflow.as_ref()).await?;
            let size = entry.message_body().len();
            if entries.len() == 10 || bytes + size > PAYLOAD_LIMIT {
                send_batch_chunk(
                    client,
                    &url,
                    std::mem::take(&mut entries),
                    &mut confirmed[start..index],
                )
                .await?;
                start = index;
                bytes = 0;
            }
            bytes += size;
            entries.push(entry);
        }
        if !entries.is_empty() {
            send_batch_chunk(client, &url, entries, &mut confirmed[start..]).await?;
        }
        Ok(())
    }

    async fn reserve(
        &self,
        queue: Option<&str>,
        wait: Option<Duration>,
    ) -> QueueResult<Option<Box<dyn Job>>> {
        let url = self.url(queue)?;
        let client = self.client().await;
        let wait = wait.map(|value| i32::try_from(value.as_secs()).unwrap_or(i32::MAX).min(20));
        let result = client
            .receive_message()
            .queue_url(&url)
            .max_number_of_messages(1)
            .set_wait_time_seconds(wait)
            .set_visibility_timeout(self.options.visibility_timeout)
            .message_system_attribute_names(MessageSystemAttributeName::ApproximateReceiveCount)
            .send()
            .await
            .map_err(backend)?;
        let Some(message) = result.messages().first() else {
            return Ok(None);
        };
        let id = message.message_id().ok_or(QueueError::InvalidPayload)?;
        let receipt = message.receipt_handle().ok_or(QueueError::InvalidPayload)?;
        let attempts = message
            .attributes()
            .and_then(|values| values.get(&MessageSystemAttributeName::ApproximateReceiveCount))
            .and_then(|value| value.parse().ok())
            .ok_or(QueueError::InvalidPayload)?;
        let body = message.body().ok_or(QueueError::InvalidPayload)?;
        let pointer = self
            .overflow
            .as_ref()
            .and_then(|_| serde_json::from_str::<Pointer>(body).ok());
        let payload = if let (Some(store), Some(pointer)) = (&self.overflow, &pointer) {
            let id = pointer
                .key
                .strip_prefix(POINTER_PREFIX)
                .ok_or(QueueError::InvalidPayload)?;
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(QueueError::InvalidPayload);
            }
            let Some(Value::Json(serde_json::Value::String(body))) =
                store.cache.get(&pointer.key).await.map_err(backend)?
            else {
                return Err(QueueError::InvalidPayload);
            };
            Payload::decode(&body)?
        } else {
            Payload::decode(body)?
        };
        Ok(Some(Box::new(ReservedJob {
            registry: self.registry.clone(),
            storage: Box::new(SqsReservation {
                client: client.clone(),
                options: self.options.clone(),
                url: url.clone(),
                receipt: receipt.into(),
                overflow: self.overflow.clone(),
                pointer,
            }),
            id: id.into(),
            context: JobContext {
                connection: self.name.clone(),
                queue: url,
                attempts,
                ..Default::default()
            },
            payload,
            deleted: false,
            released: false,
            failed: false,
        })))
    }
}

impl Contract for SqsQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }
    fn default_queue(&self) -> &str {
        &self.queue
    }
    fn clearable(&self) -> Option<&dyn ClearableQueue> {
        Some(self)
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move { Ok(self.metrics(queue).await?.size) })
    }

    fn metrics<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async move {
            let url = self.url(queue)?;
            let result = self
                .client()
                .await
                .get_queue_attributes()
                .queue_url(url)
                .attribute_names(QueueAttributeName::ApproximateNumberOfMessages)
                .attribute_names(QueueAttributeName::ApproximateNumberOfMessagesDelayed)
                .attribute_names(QueueAttributeName::ApproximateNumberOfMessagesNotVisible)
                .send()
                .await
                .map_err(backend)?;
            let count = |key| -> QueueResult<u64> {
                result
                    .attributes()
                    .and_then(|values| values.get(&key))
                    .map_or(Ok(0), |value| value.parse().map_err(backend))
            };
            let pending = count(QueueAttributeName::ApproximateNumberOfMessages)?;
            let delayed = count(QueueAttributeName::ApproximateNumberOfMessagesDelayed)?;
            let reserved = count(QueueAttributeName::ApproximateNumberOfMessagesNotVisible)?;
            Ok(QueueMetrics {
                size: pending.saturating_add(delayed).saturating_add(reserved),
                pending: Some(pending),
                delayed: Some(delayed),
                reserved: Some(reserved),
                oldest_pending: None,
            })
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.send(payload, queue, None))
    }
    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.send(payload, queue, Some(delay)))
    }
    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(
            self.reserve(
                queue,
                self.options
                    .block_for
                    .map(|value| Duration::from_secs(u64::from(value))),
            ),
        )
    }
    fn pop_now<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.reserve(queue, Some(Duration::ZERO)))
    }
    fn pop_within<'a>(
        &'a self,
        queue: Option<&'a str>,
        wait: Duration,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        let wait = self.options.block_for.map_or(wait, |value| {
            wait.min(Duration::from_secs(u64::from(value)))
        });
        Box::pin(self.reserve(queue, Some(wait)))
    }
}

impl ClearableQueue for SqsQueue {
    fn clear<'a>(&'a self, queue: &'a str) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move {
            let size = self.size(Some(queue)).await?;
            self.client()
                .await
                .purge_queue()
                .queue_url(self.url(Some(queue))?)
                .send()
                .await
                .map_err(backend)?;
            if let Some(store) = &self.overflow
                && store.flush_on_clear
            {
                store.cache.flush().await.map_err(backend)?;
            }
            Ok(size)
        })
    }
}

struct SqsReservation {
    client: Client,
    options: Arc<Options>,
    url: String,
    receipt: String,
    overflow: Option<OverflowStore>,
    pointer: Option<Pointer>,
}

impl Reservation for SqsReservation {
    fn advance_chain<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let url = queue
                .map(|name| queue_url(name, &self.options))
                .transpose()?;
            send(
                &self.client,
                url.as_deref().unwrap_or(&self.url),
                next,
                delay,
                true,
                self.overflow.as_ref(),
            )
            .await?;
            self.delete().await
        })
    }

    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            self.client
                .delete_message()
                .queue_url(&self.url)
                .receipt_handle(&self.receipt)
                .send()
                .await
                .map_err(settlement_error)?;
            if let (Some(store), Some(pointer)) = (&self.overflow, &self.pointer)
                && store.delete_after_processing
            {
                store.cache.forget(&pointer.key).await.map_err(backend)?;
            }
            Ok(())
        })
    }
    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            self.client
                .change_message_visibility()
                .queue_url(&self.url)
                .receipt_handle(&self.receipt)
                .visibility_timeout(seconds(delay, 43200)?)
                .send()
                .await
                .map_err(settlement_error)?;
            Ok(())
        })
    }
}

async fn send(
    client: &Client,
    url: &str,
    payload: &Payload,
    delay: Option<Delay>,
    stable_deduplication: bool,
    overflow: Option<&OverflowStore>,
) -> QueueResult<Option<String>> {
    let entry = prepare_message(url, payload, delay, stable_deduplication, overflow).await?;
    // A send error can mean the service accepted the message without returning a response.
    let result = client
        .send_message()
        .queue_url(url)
        .message_body(entry.message_body)
        .set_message_group_id(entry.message_group_id)
        .set_message_deduplication_id(entry.message_deduplication_id)
        .set_delay_seconds(entry.delay_seconds)
        .send()
        .await
        .map_err(backend)?;
    Ok(Some(
        result
            .message_id()
            .ok_or(QueueError::InvalidPayload)?
            .into(),
    ))
}

async fn send_batch_chunk(
    client: &Client,
    url: &str,
    mut entries: Vec<SendMessageBatchRequestEntry>,
    confirmed: &mut [bool],
) -> QueueResult<()> {
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.id = index.to_string();
    }
    let result = client
        .send_message_batch()
        .queue_url(url)
        .set_entries(Some(entries))
        .send()
        .await
        .map_err(backend)?;
    for success in result.successful() {
        let Some(slot) = success
            .id()
            .parse::<usize>()
            .ok()
            .and_then(|index| confirmed.get_mut(index))
        else {
            return Err(QueueError::InvalidPayload);
        };
        *slot = true;
    }
    if let Some(failure) = result.failed().first() {
        return Err(backend(std::io::Error::other(format!(
            "SQS rejected {} batch entries (first code: {})",
            result.failed().len(),
            failure.code(),
        ))));
    }
    if confirmed.iter().any(|sent| !sent) {
        return Err(QueueError::InvalidPayload);
    }
    Ok(())
}

async fn prepare_message(
    url: &str,
    payload: &Payload,
    delay: Option<Delay>,
    stable_deduplication: bool,
    overflow: Option<&OverflowStore>,
) -> QueueResult<SendMessageBatchRequestEntry> {
    let body = serde_json::to_string(payload).map_err(backend)?;
    let overflow = overflow.filter(|store| store.always || body.len() >= PAYLOAD_LIMIT);
    if overflow.is_none() && body.len() > PAYLOAD_LIMIT {
        return Err(QueueError::InvalidPayload);
    }
    let mut command = SendMessageBatchRequestEntry::builder().id("0");
    if url.ends_with(".fifo") {
        let deduplication = match &payload.sqs_deduplication {
            value
                if value.as_ref().map_or(stable_deduplication, |id| {
                    id.is_empty() && overflow.is_some()
                }) =>
            {
                // Retries and overflow use the original body rather than a random wire ID.
                let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, body.as_bytes());
                Some(
                    digest
                        .as_ref()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                )
            }
            Some(value) => (!value.is_empty()).then(|| value.clone()),
            None => Some(super::new_id()?),
        };
        command = command
            .message_group_id(
                payload
                    .sqs_group
                    .as_deref()
                    .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(url)),
            )
            .set_message_deduplication_id(deduplication);
    } else {
        command = command.set_message_group_id(payload.sqs_group.clone());
        if let Some(delay) = delay {
            let duration = match delay {
                Delay::For(duration) => duration,
                Delay::Until(time) => time.duration_since(SystemTime::now()).unwrap_or_default(),
            };
            command = command.delay_seconds(seconds(duration, 900)?);
        }
    }
    let body = if let Some(store) = overflow {
        let key = format!("{POINTER_PREFIX}{}", super::new_id()?);
        if !store
            .cache
            .put(&key, &Value::Json(body.into()), store.ttl)
            .await
            .map_err(backend)?
        {
            return Err(QueueError::InvalidPayload);
        }
        serde_json::to_string(&Pointer { key }).map_err(backend)?
    } else {
        body
    };
    command.message_body(body).build().map_err(backend)
}

fn seconds(duration: Duration, max: i32) -> QueueResult<i32> {
    let value = duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0));
    i32::try_from(value)
        .ok()
        .filter(|value| *value <= max)
        .ok_or(QueueError::InvalidConfiguration)
}

fn settlement_error<E>(error: aws_sdk_sqs::error::SdkError<E>) -> QueueError
where
    E: aws_sdk_sqs::error::ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    if error.as_service_error().is_some_and(|cause| {
        matches!(
            cause.code(),
            Some("ReceiptHandleIsInvalid" | "MessageNotInflight" | "MessageNotInFlight")
        )
    }) {
        QueueError::LostReservation
    } else {
        backend(error)
    }
}
