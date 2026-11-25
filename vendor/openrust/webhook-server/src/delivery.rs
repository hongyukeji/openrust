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
    http::{
        HttpFuture,
        client::{Factory, Response},
    },
    log::Log,
};
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::{
    DeliveryProgress, DeliveryStore, Error, Result, TargetPolicy, WebhookJobAttempt,
    delivery_store::{AttemptResult, Claim, RetryAfter},
    events::{self, WebhookAttempt, WebhookCallEvent},
    extensions, limits, replay,
    snapshot::Snapshot,
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
        self.run(id, None).await.map(|(progress, _)| progress)
    }

    pub(super) async fn run(
        &self,
        id: &str,
        expected_job: Option<&str>,
    ) -> Result<(DeliveryProgress, bool)> {
        let started = Instant::now();
        let claimed = self
            .store
            .claim(id, |snapshot| {
                if expected_job.is_some_and(|name| name != snapshot.job) {
                    return Err(Error::InvalidConfig("delivery.job"));
                }
                snapshot
                    .custom_job
                    .then(|| extensions::call_webhook_job(&snapshot.job))
                    .transpose()
            })
            .await;
        let (claim, handler) = match claimed {
            Ok(Some(claim)) => claim,
            Ok(None) | Err(Error::EndpointPaused) => {
                return Ok((DeliveryProgress::Unavailable, false));
            }
            Err(error) => return Err(error),
        };
        let remaining = u64::try_from(claim.deadline - claim.timestamp).unwrap_or(0);
        let duration = Duration::from_secs(remaining.min(u64::from(claim.snapshot.timeout)));
        let deadline = started + duration;
        let TransportAttempt {
            result: attempt,
            response,
            retry_at,
        } = match tokio::time::timeout_at(deadline, self.attempt(id, &claim, deadline)).await {
            Ok(outcome) => outcome,
            Err(_) => AttemptResult::transient("unknown").into(),
        };
        if attempt.outcome == "paused" || retry_at.is_some() {
            self.store.defer_claim(id, &claim, retry_at).await?;
            return Ok((DeliveryProgress::Pending, false));
        }
        let status = attempt.status;
        let outcome = attempt.outcome;
        let observed = WebhookJobAttempt {
            scope: self.store.scope(),
            delivery_id: id,
            number: claim.number,
            outcome,
            snapshot: &claim.snapshot,
            response: response.as_ref(),
        };
        let backoff = if attempt.retry_after.is_some()
            && claim.number < claim.snapshot.tries
            && !handler
                .as_ref()
                .is_some_and(|job| job.should_be_removed_from_queue(&observed))
        {
            retry_delay(&claim)
        } else {
            None
        };
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
        events::attempted(event.clone(), progress).await;
        if let Some(job) = handler
            && progress.is_final_failure()
            && job
                .failed(&WebhookJobAttempt {
                    scope: self.store.scope(),
                    delivery_id: id,
                    number: claim.number,
                    outcome,
                    snapshot: &event.snapshot,
                    response: response.as_ref(),
                })
                .is_err()
        {
            let _ = Log::warning("Webhook 发送 Job 的失败钩子执行失败", &Default::default());
        }
        Ok((progress, throw))
    }

    pub(crate) async fn check_replay_destination(
        &self,
        id: &str,
        snapshot: &Snapshot,
    ) -> Result<()> {
        self.store.check_endpoint(snapshot).await?;
        let permission = self
            .policy
            .authorize(DeliveryContext {
                scope: self.store.scope(),
                delivery_id: id,
                message_id: &snapshot.uuid,
                url: &snapshot.url,
                credential: snapshot.credential.as_deref(),
            })
            .await
            .map_err(|_| Error::ReplayDenied)?
            .ok_or(Error::ReplayDenied)?;
        let url =
            crate::destination::parse_target(&snapshot.url).map_err(|_| Error::ReplayDenied)?;
        permission
            .target
            .validate_target(&url)
            .map_err(|_| Error::ReplayDenied)?;
        if snapshot.credential.is_some() && permission.secret.is_none() {
            return Err(Error::ReplayDenied);
        }
        Ok(())
    }

    async fn endpoint_stop(&self, snapshot: &Snapshot) -> Option<AttemptResult> {
        match self.store.check_endpoint(snapshot).await {
            Ok(()) => None,
            Err(Error::EndpointPaused) => Some(AttemptResult::failure("paused")),
            Err(Error::EndpointRevoked) => Some(AttemptResult::failure("cancelled")),
            Err(_) => Some(AttemptResult::transient("unresolved")),
        }
    }

    async fn attempt(&self, id: &str, claim: &Claim, deadline: Instant) -> TransportAttempt {
        let snapshot = &claim.snapshot;
        if let Some(result) = self.endpoint_stop(snapshot).await {
            return result.into();
        }
        if snapshot.replay {
            let record = match self.store.replay_record(id).await {
                Ok(record) => record,
                Err(Error::Database) => return AttemptResult::transient("unresolved").into(),
                Err(_) => return AttemptResult::failure("rejected").into(),
            };
            if replay::authorize_attempt(self.store.scope(), &record, snapshot)
                .await
                .is_err()
            {
                return AttemptResult::failure("rejected").into();
            }
        }
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
            return AttemptResult::failure("rejected").into();
        };
        let target = match permission.target.resolve(&snapshot.url).await {
            Ok(target) => target,
            Err(Error::TargetResolutionFailed) => {
                return AttemptResult::transient("unresolved").into();
            }
            Err(_) => return AttemptResult::failure("rejected").into(),
        };
        let secret = permission.secret.as_ref().map(|value| value.as_str());
        let Ok((method, headers, body)) = snapshot.request_parts(claim.timestamp, secret) else {
            return AttemptResult::failure("rejected").into();
        };
        let Ok(factory) = Factory::new() else {
            return AttemptResult::failure("rejected").into();
        };
        let url = target.url().as_str();
        let Ok(request) = factory
            .pending_request()
            .pin_destination(url, target.addresses().to_vec())
        else {
            return AttemptResult::failure("rejected").into();
        };
        if let Some(result) = self.endpoint_stop(snapshot).await {
            return result.into();
        }
        let Ok(limits) = limits::current() else {
            return AttemptResult::transient("unresolved").into();
        };
        if limits.sends_per_scope.is_some()
            || limits.sends_per_endpoint.is_some() && snapshot.endpoint.is_some()
        {
            match self.store.reserve_send(id, claim, limits).await {
                Ok(()) => {}
                Err(Error::RateLimited { retry_at }) => {
                    return TransportAttempt {
                        result: AttemptResult::failure("rate_limited"),
                        response: None,
                        retry_at: Some(retry_at),
                    };
                }
                Err(_) => return AttemptResult::transient("unresolved").into(),
            }
            if let Some(result) = self.endpoint_stop(snapshot).await {
                return result.into();
            }
        }
        let duration = deadline.saturating_duration_since(Instant::now());
        if duration.is_zero() {
            return AttemptResult::transient("stopped").into();
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
                let result = AttemptResult {
                    outcome: if status.is_success() {
                        "succeeded"
                    } else {
                        "http_failure"
                    },
                    status: Some(status.as_u16()),
                    retry_after,
                };
                TransportAttempt {
                    result,
                    response: Some(response),
                    retry_at: None,
                }
            }
            Err(_) => AttemptResult::transient("unknown").into(),
        }
    }
}

struct TransportAttempt {
    result: AttemptResult,
    response: Option<Response>,
    retry_at: Option<i64>,
}

impl From<AttemptResult> for TransportAttempt {
    fn from(result: AttemptResult) -> Self {
        Self {
            result,
            response: None,
            retry_at: None,
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
