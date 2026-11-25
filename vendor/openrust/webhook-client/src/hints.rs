//! 显式启用的无签名查单提示与可信接收记录分开处理。
use crate::{Error, Result, WebhookConfig, WebhookRequest, factories::validate_reference};
use openrust::{
    cache::RateLimiter,
    http::HttpFuture,
    queue::{DispatchOptions, JobOptions, Payload, QueueFacade, QueueManager},
};
use std::{sync::Arc, time::Duration};

mod storage;
mod worker;
use storage::HintStore;

/// 仅在接收端按协议确认签名字段完全缺失时产生。
pub struct UnverifiedHint<'a> {
    request: &'a WebhookRequest,
    config: &'a WebhookConfig,
}

impl<'a> UnverifiedHint<'a> {
    pub(crate) fn new(request: &'a WebhookRequest, config: &'a WebhookConfig) -> Self {
        Self { request, config }
    }

    pub fn request(&self) -> &WebhookRequest {
        self.request
    }

    pub fn profile(&self) -> &str {
        self.config.name()
    }

    pub fn revision(&self) -> &str {
        self.config.revision()
    }
}

pub trait ReceivesUnverifiedHint: Send + Sync {
    fn receive<'a>(&'a self, hint: UnverifiedHint<'a>) -> HttpFuture<'a, Result<()>>;
}

/// 把不可信请求关联到同一账户已登记的本地任务；未知任务返回 None。
pub trait WebhookHintPolicy: Send + Sync {
    fn resolve<'a>(
        &'a self,
        scope: &'a str,
        hint: UnverifiedHint<'a>,
    ) -> HttpFuture<'a, Result<Option<String>>>;
}

/// 给查单任务使用的最小快照，由独立提示存储恢复。
pub struct WebhookHint {
    id: String,
    scope: String,
    profile: String,
    revision: String,
    task_id: String,
}

impl WebhookHint {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn scope(&self) -> &str {
        &self.scope
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
}

pub trait ProcessWebhookHint: Send + Sync {
    /// Worker 查单前重新确认该范围和修订中的本地任务仍然有效。
    fn is_registered<'a>(&'a self, hint: &'a WebhookHint) -> HttpFuture<'a, Result<bool>>;
    fn handle<'a>(&'a self, hint: &'a WebhookHint) -> HttpFuture<'a, Result<()>>;
}

pub struct QueuedWebhookHint {
    store: HintStore,
    job: String,
    destination: DispatchOptions,
}

impl QueuedWebhookHint {
    pub fn new(
        database: openrust::database::Connection,
        scope: &str,
        profile: &str,
        job: &str,
        connection: &str,
        queue: &str,
    ) -> Result<Self> {
        validate_reference("unverified_hint_job", job)?;
        if !valid_identifier(scope) || !valid_identifier(profile) {
            return Err(Error::InvalidConfig("unverified_hint_scope"));
        }
        Ok(Self {
            store: HintStore::new(database, scope, profile),
            job: format!("openrust_webhook_client::hints::{job}"),
            destination: DispatchOptions {
                connection: Some(connection.to_owned()),
                queue: Some(queue.to_owned()),
                ..Default::default()
            },
        })
    }

    pub fn register(
        &self,
        queue: &QueueManager,
        handler: Arc<dyn ProcessWebhookHint>,
    ) -> Result<()> {
        Ok(queue.register(
            &self.job,
            Arc::new(worker::HintJob::new(self.store.clone(), handler)),
        )?)
    }

    pub fn receiver(
        self,
        limiter: Arc<RateLimiter>,
        per_minute: u32,
        policy: Arc<dyn WebhookHintPolicy>,
    ) -> Result<impl ReceivesUnverifiedHint> {
        if !(1..=600).contains(&per_minute) {
            return Err(Error::InvalidConfig("unverified_hint_per_minute"));
        }
        Ok(HintReceiver {
            job: self,
            limiter,
            per_minute,
            policy,
        })
    }
}

struct HintReceiver {
    job: QueuedWebhookHint,
    limiter: Arc<RateLimiter>,
    per_minute: u32,
    policy: Arc<dyn WebhookHintPolicy>,
}

impl ReceivesUnverifiedHint for HintReceiver {
    fn receive<'a>(&'a self, hint: UnverifiedHint<'a>) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            let store = &self.job.store;
            if hint.profile() != store.profile() {
                return Err(Error::InvalidConfig("unverified_hint_profile"));
            }
            if hint.config.options()["scope"].as_str() != Some(store.scope()) {
                return Err(Error::InvalidConfig("unverified_hint_scope"));
            }
            let count = self
                .limiter
                .hit(&store.limiter_key(), 60)
                .await
                .map_err(|source| Error::Runtime {
                    source: Box::new(source),
                })?;
            if count > u64::from(self.per_minute) {
                return Err(openrust::http::abort(
                    openrust::http::StatusCode::TOO_MANY_REQUESTS,
                    "Webhook 无签名提示超过限额",
                    Default::default(),
                )
                .into());
            }
            let revision = hint.revision().to_owned();
            let Some(task) = self.policy.resolve(store.scope(), hint).await? else {
                return Ok(());
            };
            if !valid_identifier(&task) {
                return Err(Error::InvalidConfig("unverified_hint_task"));
            }
            let record = store.new_hint(revision, task)?;
            let manager = QueueFacade::manager()?;
            if !manager.has_handler(&self.job.job) {
                return Err(openrust::queue::QueueError::UnknownJob(self.job.job.clone()).into());
            }
            let task = Payload::new(&self.job.job, wire::Receipt::from(&record))?.with_options(
                JobOptions {
                    max_tries: Some(1),
                    timeout: Some(Duration::from_secs(15)),
                    ..Default::default()
                },
            );
            store
                .save_and_enqueue(record, manager, task, self.job.destination.clone())
                .await
        })
    }
}

mod wire {
    use super::WebhookHint;

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Receipt<'a> {
        pub id: &'a str,
        pub revision: &'a str,
    }

    impl<'a> From<&'a WebhookHint> for Receipt<'a> {
        fn from(hint: &'a WebhookHint) -> Self {
            Self {
                id: &hint.id,
                revision: &hint.revision,
            }
        }
    }
}

pub(super) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
