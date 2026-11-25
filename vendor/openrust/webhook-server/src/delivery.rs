use std::{sync::Arc, time::Duration};

use http::{
    HeaderMap, HeaderValue, Method,
    header::{CONTENT_TYPE, RETRY_AFTER},
};
use openrust::{
    container::Container,
    contracts::container::Container as _,
    http::{
        HttpFuture,
        client::{Factory, PinnedProxy, Response, RetryAfter, TlsCertificate, TlsIdentity},
    },
    log::Log,
};
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::{
    DeliveryProgress, DeliveryStore, Error, Result, TargetPolicy, WebhookJobAttempt,
    delivery_store::{AttemptResult, Claim},
    events::{self, WebhookAttempt, WebhookCallEvent},
    extensions, limits, replay,
    snapshot::Snapshot,
};

const MAX_TLS_PEM_BYTES: usize = 128 * 1024;

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
    tls_ca: Option<Vec<TlsCertificate>>,
    tls_identity: Option<TlsIdentity>,
    allow_unverified_tls: bool,
    proxy: Option<(String, PinnedProxy)>,
}

impl DeliveryPermission {
    pub fn new(target: TargetPolicy) -> Self {
        Self {
            target,
            secret: None,
            tls_ca: None,
            tls_identity: None,
            allow_unverified_tls: false,
            proxy: None,
        }
    }

    pub fn with_secret(mut self, secret: impl Into<Zeroizing<String>>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// 以应用为当前目的地批准的 CA 链替换系统信任库。
    pub fn with_tls_ca(mut self, bundle: &[u8]) -> Result<Self> {
        if bundle.len() > MAX_TLS_PEM_BYTES {
            return Err(Error::InvalidConfig("delivery.tls_ca"));
        }
        let certificates = TlsCertificate::from_pem_bundle(bundle)
            .map_err(|_| Error::InvalidConfig("delivery.tls_ca"))?;
        if certificates.is_empty() {
            return Err(Error::InvalidConfig("delivery.tls_ca"));
        }
        self.tls_ca = Some(certificates);
        Ok(self)
    }

    /// 从应用加载的 PEM 证书链及私钥建立当前目的地的客户端身份。
    pub fn with_mutual_tls(mut self, certificate: &[u8], key: &[u8]) -> Result<Self> {
        if certificate.is_empty()
            || key.is_empty()
            || certificate.len().saturating_add(key.len()) > MAX_TLS_PEM_BYTES
        {
            return Err(Error::InvalidConfig("delivery.tls_identity"));
        }
        let mut identity = Zeroizing::new(Vec::with_capacity(certificate.len() + key.len() + 1));
        identity.extend_from_slice(certificate);
        identity.push(b'\n');
        identity.extend_from_slice(key);
        self.tls_identity = Some(
            TlsIdentity::from_pem(&identity)
                .map_err(|_| Error::InvalidConfig("delivery.tls_identity"))?,
        );
        Ok(self)
    }

    /// 仅在隔离环境中许可显式关闭服务端证书和主机名校验的投递。
    pub fn allow_unverified_tls(mut self) -> Self {
        self.allow_unverified_tls = true;
        self
    }

    /// 为当前授权范围和目的地提供具名代理。认证材料保留在调用进程。
    pub fn with_proxy(
        mut self,
        reference: impl Into<String>,
        proxy: impl Into<PinnedProxy>,
    ) -> Result<Self> {
        let reference = reference.into();
        extensions::validate_name("delivery.proxy", &reference)?;
        self.proxy = Some((reference, proxy.into()));
        Ok(self)
    }

    fn permits_transport(&self, snapshot: &Snapshot, https: bool) -> bool {
        if let Some(reference) = &snapshot.proxy
            && (!https
                || snapshot.unverified_tls
                || self
                    .proxy
                    .as_ref()
                    .is_none_or(|(name, _)| name != reference))
        {
            return false;
        }
        let uses_credentials = self.tls_ca.is_some() || self.tls_identity.is_some();
        if snapshot.unverified_tls {
            https && !uses_credentials && self.allow_unverified_tls
        } else {
            !uses_credentials || https
        }
    }
}

/// 由应用根据当前配置核对范围、目的地与凭据引用。
pub trait DeliveryPolicy: Send + Sync {
    fn authorize<'a>(
        &'a self,
        context: DeliveryContext<'a>,
    ) -> HttpFuture<'a, Result<Option<DeliveryPermission>>>;

    /// 应用可按所请求的代理名称进一步授权投递。
    fn authorize_transport<'a>(
        &'a self,
        context: DeliveryContext<'a>,
        _proxy: Option<&'a str>,
    ) -> HttpFuture<'a, Result<Option<DeliveryPermission>>> {
        self.authorize(context)
    }
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
            .authorize_transport(
                DeliveryContext {
                    scope: self.store.scope(),
                    delivery_id: id,
                    message_id: &snapshot.uuid,
                    url: &snapshot.url,
                    credential: snapshot.credential.as_deref(),
                },
                snapshot.proxy.as_deref(),
            )
            .await
            .map_err(|_| Error::ReplayDenied)?
            .ok_or(Error::ReplayDenied)?;
        let url =
            crate::destination::parse_target(&snapshot.url).map_err(|_| Error::ReplayDenied)?;
        permission
            .target
            .validate_target(&url)
            .map_err(|_| Error::ReplayDenied)?;
        if !permission.permits_transport(snapshot, url.scheme() == "https")
            || snapshot.credential.is_some() && permission.secret.is_none()
        {
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
            .authorize_transport(
                DeliveryContext {
                    scope: self.store.scope(),
                    delivery_id: id,
                    message_id: &snapshot.uuid,
                    url: &snapshot.url,
                    credential: snapshot.credential.as_deref(),
                },
                snapshot.proxy.as_deref(),
            )
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
        if !permission.permits_transport(snapshot, target.url().scheme() == "https") {
            return AttemptResult::failure("rejected").into();
        }
        let secret = permission.secret.as_ref().map(|value| value.as_str());
        let Ok((method, headers, body)) = snapshot.request_parts(claim.timestamp, secret) else {
            return AttemptResult::failure("rejected").into();
        };
        let Ok(factory) = Factory::new() else {
            return AttemptResult::failure("rejected").into();
        };
        let url = target.url().as_str();
        let Ok(mut request) = factory
            .pending_request()
            .pin_destination(url, target.addresses().to_vec())
        else {
            return AttemptResult::failure("rejected").into();
        };
        if let Some(result) = self.endpoint_stop(snapshot).await {
            return result.into();
        }
        if let Some(ca) = permission.tls_ca {
            request = request.with_tls_ca(ca);
        }
        if let Some(identity) = permission.tls_identity {
            request = request.with_tls_identity(identity);
        }
        if snapshot.unverified_tls {
            request = request.without_verifying();
        }
        if snapshot.proxy.is_some()
            && let Some((_, proxy)) = permission.proxy
        {
            request = request.through_pinned_proxy(proxy);
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
            Err(openrust::http::client::Error::Configuration) => {
                AttemptResult::failure("rejected").into()
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
    RetryAfter::from_header(headers.get(RETRY_AFTER)?)
}
