use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use http::{
    HeaderMap, HeaderValue, Method,
    header::{CONTENT_TYPE, RETRY_AFTER},
};
use openrust::{
    container::Container,
    contracts::container::Container as _,
    http::{HttpFuture, client::Factory},
};
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::{
    DeliveryProgress, DeliveryStore, Error, Result, TargetPolicy,
    delivery_store::{AttemptResult, Claim, RetryAfter},
    events::{self, WebhookAttempt, WebhookCallEvent},
    extensions,
};

/// 每次领取后交给应用重新授权的投递信息。
pub struct DeliveryContext<'a> {
    pub scope: &'a str,
    pub delivery_id: &'a str,
    pub message_id: &'a str,
    pub url: &'a str,
    pub credential: Option<&'a str>,
}

pub struct DeliveryPermission {
    target: TargetPolicy,
    secret: Option<Zeroizing<String>>,
}

impl DeliveryPermission {
    pub fn new(target: TargetPolicy) -> Self {
        Self {
            target,
            secret: None,
        }
    }

    pub fn with_secret(mut self, secret: impl Into<Zeroizing<String>>) -> Self {
        self.secret = Some(secret.into());
        self
    }
}

/// 由应用根据当前配置核对范围、目的地与凭据引用。
pub trait DeliveryPolicy: Send + Sync {
    fn authorize<'a>(
        &'a self,
        context: DeliveryContext<'a>,
    ) -> HttpFuture<'a, Result<Option<DeliveryPermission>>>;
}

pub(crate) struct RegisteredDeliveryPolicy(pub Arc<dyn DeliveryPolicy>);

pub fn register_delivery_policy(
    container: &Container,
    policy: Arc<dyn DeliveryPolicy>,
) -> Result<()> {
    container
        .instance(Arc::new(RegisteredDeliveryPolicy(policy)))
        .map(|_| ())
        .map_err(|_| Error::InvalidConfig("delivery.policy"))
}

#[derive(Clone)]
pub struct DeliveryService {
    pub(super) store: DeliveryStore,
    policy: Arc<dyn DeliveryPolicy>,
}

impl DeliveryService {
    pub fn new(store: DeliveryStore, policy: Arc<dyn DeliveryPolicy>) -> Self {
        Self { store, policy }
    }

    /// 同步尝试一次。后续尝试由持久投递记录管理。
    pub async fn dispatch_sync(&self, id: &str) -> Result<DeliveryProgress> {
        self.run(id).await.map(|(progress, _)| progress)
    }

    pub(super) async fn run(&self, id: &str) -> Result<(DeliveryProgress, bool)> {
        let started = Instant::now();
        let Some(claim) = self.store.claim(id).await? else {
            return Ok((DeliveryProgress::Unavailable, false));
        };
        let remaining = u64::try_from(claim.deadline - claim.timestamp).unwrap_or(0);
        let duration = Duration::from_secs(remaining.min(u64::from(claim.snapshot.timeout)));
        let deadline = started + duration;
        let attempt =
            match tokio::time::timeout_at(deadline, self.attempt(id, &claim, deadline)).await {
                Ok(outcome) => outcome,
                Err(_) => AttemptResult::transient("unknown"),
            };
        let backoff = if attempt.retry_after.is_some() && claim.number < claim.snapshot.tries {
            retry_delay(&claim)
        } else {
            None
        };
        let status = attempt.status;
        let outcome = attempt.outcome;
        let elapsed = started.elapsed();
        let progress = self.store.finish(id, &claim, attempt, backoff).await?;
        let throw = claim.snapshot.throws_on_failure;
        let event = WebhookCallEvent::new(
            self.store.scope(),
            id,
            claim.snapshot,
            Some(WebhookAttempt {
                number: claim.number,
                status,
                outcome,
                progress,
                elapsed,
            }),
        );
        events::attempted(event, progress).await;
        Ok((progress, throw))
    }

    async fn attempt(&self, id: &str, claim: &Claim, deadline: Instant) -> AttemptResult {
        let snapshot = &claim.snapshot;
        let permission = self
            .policy
            .authorize(DeliveryContext {
                scope: self.store.scope(),
                delivery_id: id,
                message_id: &snapshot.uuid,
                url: &snapshot.url,
                credential: snapshot.credential.as_deref(),
            })
            .await;
        let Ok(Some(permission)) = permission else {
            return AttemptResult::failure("rejected");
        };
        let target = match permission.target.resolve(&snapshot.url).await {
            Ok(target) => target,
            Err(Error::TargetResolutionFailed) => return AttemptResult::transient("unresolved"),
            Err(_) => return AttemptResult::failure("rejected"),
        };
        let secret = permission.secret.as_ref().map(|value| value.as_str());
        let Ok((method, headers, body)) = snapshot.request_parts(claim.timestamp, secret) else {
            return AttemptResult::failure("rejected");
        };
        let Ok(factory) = Factory::new() else {
            return AttemptResult::failure("rejected");
        };
        let url = target.url().as_str();
        let Ok(request) = factory
            .pending_request()
            .pin_destination(url, target.addresses().to_vec())
        else {
            return AttemptResult::failure("rejected");
        };
        let duration = deadline.saturating_duration_since(Instant::now());
        if duration.is_zero() {
            return AttemptResult::transient("stopped");
        }
        let content_type = headers.get(CONTENT_TYPE).cloned();
        let request = request
            .with_headers(headers)
            .timeout(duration)
            .max_response_bytes(64 * 1024);
        let request = if method == Method::GET {
            request
        } else {
            request.with_body(
                body,
                content_type
                    .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream")),
            )
        };
        match request.send(method, url).await {
            Ok(response) => {
                let status = response.status();
                let retry_after = snapshot.retries_status(status).then(|| {
                    snapshot
                        .respect_retry_after
                        .then(|| retry_after(response.headers()))
                        .flatten()
                        .unwrap_or(RetryAfter::Seconds(0))
                });
                AttemptResult {
                    outcome: if status.is_success() {
                        "succeeded"
                    } else {
                        "http_failure"
                    },
                    status: Some(status.as_u16()),
                    retry_after,
                }
            }
            Err(_) => AttemptResult::transient("unknown"),
        }
    }
}

fn retry_delay(claim: &Claim) -> Option<u64> {
    let strategy = extensions::backoff(&claim.snapshot.backoff).ok()?;
    let seconds = strategy.wait_in_seconds_after_attempt(claim.number);
    Some(if claim.snapshot.jitter {
        fastrand::u64(0..=seconds)
    } else {
        seconds
    })
}

fn retry_after(headers: &HeaderMap) -> Option<RetryAfter> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value
            .parse::<i64>()
            .ok()
            .map(|seconds| RetryAfter::Seconds(seconds as u64));
    }
    let date = httpdate::parse_http_date(value)
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?;
    Some(RetryAfter::Timestamp(i64::try_from(date.as_secs()).ok()?))
}
