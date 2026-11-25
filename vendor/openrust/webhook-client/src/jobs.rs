//! 通过原生队列在独立 Worker 中重新读取接收记录。
use crate::{
    DatabaseWebhookStore,
    storage::{DeferredWakeup, ProcessingClaim, ProcessingOutcome},
};
use crate::{Error, Result, WebhookJobDispatcher, WebhookStore, models::WebhookCall};
mod recovery;
use openrust::{
    bus::Bus,
    contracts::queue::JobHandler,
    http::HttpFuture,
    queue::{
        Delay, DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError,
        QueueFacade, QueueFuture, QueueManager, QueueResult,
    },
};
pub use recovery::{DispatchRecoveryReport, RecoveryDispatch};
use std::{sync::Arc, time::Duration};

/// 应用业务处理器从其注册的存储读取载荷及附件。
pub trait ProcessWebhookJob: Send + Sync {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        call: &'a WebhookCall,
        store: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, Result<()>>;

    /// 有租约的注册调用此方法。旧处理器的成功／失败由默认实现映射。
    fn handle_outcome<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        call: &'a WebhookCall,
        store: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, Result<WebhookProcessingOutcome>> {
        Box::pin(async move {
            self.handle(context, call, store).await?;
            Ok(WebhookProcessingOutcome::Completed)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebhookProcessingOutcome {
    Completed,
    Deferred { delay_seconds: u32 },
    RetryableFailure,
    PermanentFailure,
}

/// 业务延期预算独立于每个原生队列任务的尝试次数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeferredProcessingPolicy {
    pub max_deferrals: u32,
    pub max_age_seconds: u32,
}

impl DeferredProcessingPolicy {
    fn validate(self) -> Result<Self> {
        if !(1..=1000).contains(&self.max_deferrals)
            || !(2..=2_592_000).contains(&self.max_age_seconds)
        {
            return Err(Error::InvalidConfig("deferred_processing_policy"));
        }
        Ok(self)
    }
}

/// 一个具名处理器的派发设置，连接、队列和尝试预算由原生队列执行。
pub struct QueuedWebhookJob {
    name: String,
    dispatch: DispatchOptions,
    options: JobOptions,
}

impl QueuedWebhookJob {
    pub fn new(name: &str) -> Result<Self> {
        Ok(Self {
            name: job_name(name)?,
            dispatch: DispatchOptions::default(),
            options: JobOptions::default(),
        })
    }

    pub fn on_connection(mut self, name: impl Into<String>) -> Self {
        self.dispatch.connection = Some(name.into());
        self
    }

    pub fn on_queue(mut self, name: impl Into<String>) -> Self {
        self.dispatch.queue = Some(name.into());
        self
    }

    pub fn with_options(mut self, options: JobOptions) -> Self {
        self.options = options;
        self
    }

    /// 在应用及其 Worker 共用的 Provider 中注册。一个注册名绑定同一可信存储范围。
    pub fn register(
        queue: &QueueManager,
        name: &str,
        store: Arc<dyn WebhookStore>,
        handler: Arc<dyn ProcessWebhookJob>,
    ) -> Result<()> {
        queue.register(
            &job_name(name)?,
            Arc::new(RegisteredJob {
                store,
                handler,
                lease: None,
                deferred: None,
            }),
        )?;
        Ok(())
    }
    /// 以数据库记录保护业务处理，可由多个共享数据库的 Worker 竞争。
    /// 有租约的任务必须设置比租约短至少两秒的显式超时。
    pub fn register_with_database_lease(
        queue: &QueueManager,
        name: &str,
        store: Arc<DatabaseWebhookStore>,
        handler: Arc<dyn ProcessWebhookJob>,
        lease_seconds: u32,
    ) -> Result<()> {
        Self::register_leased(queue, job_name(name)?, store, handler, lease_seconds, None)
    }

    /// 延期时在同一数据库事务中记录业务状态并创建下次原生队列任务。
    pub fn register_with_database_deferred_processing(
        queue: &QueueManager,
        name: &str,
        store: Arc<DatabaseWebhookStore>,
        handler: Arc<dyn ProcessWebhookJob>,
        lease_seconds: u32,
        policy: DeferredProcessingPolicy,
    ) -> Result<()> {
        Self::register_leased(
            queue,
            job_name(name)?,
            store,
            handler,
            lease_seconds,
            Some(policy.validate()?),
        )
    }

    fn register_leased(
        queue: &QueueManager,
        name: String,
        store: Arc<DatabaseWebhookStore>,
        handler: Arc<dyn ProcessWebhookJob>,
        lease_seconds: u32,
        deferred: Option<DeferredProcessingPolicy>,
    ) -> Result<()> {
        if !(3..=86_400).contains(&lease_seconds) {
            return Err(Error::InvalidConfig("processing_lease_seconds"));
        }
        queue.register(
            &name,
            Arc::new(RegisteredJob {
                store: store.clone(),
                handler,
                lease: Some((store, lease_seconds)),
                deferred,
            }),
        )?;
        Ok(())
    }

    /// 从已有接收记录的原队列快照提前唤醒。重复或不适用的请求返回 false。
    pub async fn wake_deferred(
        store: &DatabaseWebhookStore,
        queue: Arc<QueueManager>,
        receipt_id: &str,
    ) -> Result<bool> {
        let recovery_store = store.clone();
        store
            .wake_deferred(receipt_id, move |transaction, wakeup| {
                Box::pin(async move {
                    let DeferredWakeup {
                        id,
                        scope,
                        profile,
                        revision,
                        route,
                    } = wakeup;
                    let receipt = Receipt {
                        id: &id,
                        scope: &scope,
                        profile: &profile,
                        revision: &revision,
                    };
                    receipt.validate()?;
                    let snapshot = QueueRoute::decode(&route)?;
                    if snapshot.recover_deferred {
                        return recovery_store
                            .stage_deferred_dispatch(
                                transaction,
                                &id,
                                snapshot.dispatch_encoding()?,
                            )
                            .await;
                    }
                    let payload = snapshot.payload(receipt)?;
                    snapshot.enqueue(&queue, payload, None, transaction).await
                })
            })
            .await
    }

    fn payload(&self, call: &WebhookCall) -> Result<Payload> {
        let receipt = Receipt {
            id: call.id(),
            scope: call.scope(),
            profile: call.name(),
            revision: call.revision(),
        };
        receipt.validate()?;
        Ok(Payload::new(&self.name, receipt)?.with_options(self.options.clone()))
    }
}

impl WebhookJobDispatcher for QueuedWebhookJob {
    fn recovery_route(&self) -> Result<RecoveryDispatch> {
        self.snapshot_recovery_route()
    }

    fn dispatch_in_transaction<'a>(
        &'a self,
        call: &'a WebhookCall,
        transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            let manager = QueueFacade::manager()?;
            if !manager.has_handler(&self.name) {
                return Err(QueueError::UnknownJob(self.name.clone()).into());
            }
            match manager
                .dispatch_in_database_transaction(
                    self.payload(call)?,
                    self.dispatch.clone(),
                    transaction,
                )
                .await?
            {
                DispatchResult::Queued(_) => Ok(()),
                _ => Err(QueueError::InvalidConfiguration.into()),
            }
        })
    }
    fn dispatch<'a>(&'a self, call: &'a WebhookCall) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            let manager = QueueFacade::manager()?;
            if !manager.has_handler(&self.name) {
                return Err(QueueError::UnknownJob(self.name.clone()).into());
            }
            if manager.connection_is_synchronous(self.dispatch.connection.as_deref())? {
                return Err(Error::InvalidConfig("process_webhook_job.connection"));
            }
            let payload = self.payload(call)?;
            match Bus::dispatch(payload, self.dispatch.clone(), None).await? {
                DispatchResult::Queued(_) => Ok(()),
                _ => Err(QueueError::InvalidConfiguration.into()),
            }
        })
    }
}

const JOB_PREFIX: &str = "openrust_webhook_client::jobs::";

fn job_name(name: &str) -> Result<String> {
    crate::factories::validate_reference("process_webhook_job", name)?;
    Ok(format!("{JOB_PREFIX}{name}"))
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt<'a> {
    id: &'a str,
    scope: &'a str,
    profile: &'a str,
    revision: &'a str,
}

struct RegisteredJob {
    store: Arc<dyn WebhookStore>,
    handler: Arc<dyn ProcessWebhookJob>,
    lease: Option<(Arc<DatabaseWebhookStore>, u32)>,
    deferred: Option<DeferredProcessingPolicy>,
}

impl JobHandler for RegisteredJob {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let receipt = Receipt::deserialize_from(payload)?;
            let store = self.store.as_ref();
            let mut call = WebhookCall::find(store, receipt.id)
                .await
                .map_err(queue_error)?
                .ok_or_else(|| QueueError::MissingModel(receipt.id.to_owned()))?;
            if call.id() != receipt.id
                || call.scope() != receipt.scope
                || call.name() != receipt.profile
                || call.revision() != receipt.revision
            {
                return Err(QueueError::InvalidPayload);
            }

            if let Some((database, seconds)) = &self.lease {
                return self
                    .handle_leased(context, payload, call, database, *seconds)
                    .await;
            }
            call.clear_exception(store).await.map_err(queue_error)?;
            if self.handler.handle(context, &call, store).await.is_err() {
                call.save_exception(store, processing_exception())
                    .await
                    .map_err(queue_error)?;
                // 业务错误可能携带凭据或原文，队列失败记录只保留固定分类。
                return Err(queue_error(Error::ProcessingFailed));
            }
            Ok(())
        })
    }
}

pub(crate) fn processing_exception() -> serde_json::Value {
    serde_json::json!({"code":0,"message":"Webhook 处理任务执行失败"})
}

impl RegisteredJob {
    async fn handle_leased(
        &self,
        context: &mut JobContext<'_>,
        payload: &Payload,
        call: WebhookCall,
        store: &DatabaseWebhookStore,
        seconds: u32,
    ) -> QueueResult<()> {
        let max_timeout = Duration::from_secs(u64::from(seconds - 2));
        if payload
            .options()
            .timeout
            .is_none_or(|value| value.is_zero() || value > max_timeout)
        {
            return Err(QueueError::InvalidConfiguration);
        }
        let token = match store
            .claim_processing(&call, seconds)
            .await
            .map_err(queue_error)?
        {
            ProcessingClaim::Owned(token) => token,
            ProcessingClaim::Settled | ProcessingClaim::Scheduled => return Ok(()),
            ProcessingClaim::Busy => {
                context.release(Duration::from_secs(u64::from(seconds)));
                return Ok(());
            }
        };
        let mut call = call;
        call.exception = None;
        let result = self
            .handler
            .handle_outcome(context, &call, store)
            .await
            .unwrap_or(WebhookProcessingOutcome::RetryableFailure);
        let outcome = match result {
            WebhookProcessingOutcome::Completed if context.release_delay().is_some() => {
                ProcessingOutcome::Release
            }
            WebhookProcessingOutcome::Completed => ProcessingOutcome::Complete,
            WebhookProcessingOutcome::PermanentFailure => {
                store
                    .stop_processing(&call, &token)
                    .await
                    .map_err(queue_error)?;
                context.fail_without_retry();
                return Err(queue_error(Error::ProcessingFailed));
            }
            WebhookProcessingOutcome::Deferred { delay_seconds } => {
                if let Some(policy) = self.deferred
                    && context.release_delay().is_none()
                {
                    let queued = self
                        .schedule_deferred(
                            context,
                            payload,
                            store,
                            &call,
                            &token,
                            (delay_seconds, policy),
                        )
                        .await?;
                    if queued {
                        return Ok(());
                    }
                    context.fail_without_retry();
                    return Err(queue_error(Error::ProcessingDeferredExhausted));
                }
                ProcessingOutcome::Failed
            }
            WebhookProcessingOutcome::RetryableFailure => ProcessingOutcome::Failed,
        };
        let failed = matches!(outcome, ProcessingOutcome::Failed);
        store
            .finish_processing(&call, &token, outcome)
            .await
            .map_err(queue_error)?;
        if failed {
            return Err(queue_error(Error::ProcessingFailed));
        }
        Ok(())
    }

    async fn schedule_deferred(
        &self,
        context: &JobContext<'_>,
        payload: &Payload,
        store: &DatabaseWebhookStore,
        call: &WebhookCall,
        token: &str,
        (seconds, policy): (u32, DeferredProcessingPolicy),
    ) -> QueueResult<bool> {
        let name = payload.name();
        let recovery_job = name.strip_prefix(recovery::RECOVERY_JOB_PREFIX);
        let job = recovery_job
            .or_else(|| name.strip_prefix(JOB_PREFIX))
            .ok_or(QueueError::InvalidPayload)?;
        let route = QueueRoute {
            connection: context.connection().to_owned(),
            queue: context.queue().to_owned(),
            job: job.to_owned(),
            options: payload.options().clone(),
            recover_deferred: recovery_job.is_some(),
        };
        let route_snapshot = route.encode().map_err(queue_error)?;
        if route.recover_deferred {
            let pending = route.dispatch_encoding().map_err(queue_error)?;
            let recovery_store = store.clone();
            let id = call.id().to_owned();
            return store
                .defer_processing(
                    call,
                    token,
                    seconds,
                    policy,
                    route_snapshot,
                    move |transaction| {
                        Box::pin(async move {
                            recovery_store
                                .stage_deferred_dispatch(transaction, &id, pending)
                                .await
                        })
                    },
                )
                .await
                .map_err(queue_error);
        }
        let manager = QueueFacade::manager()?;
        let next = route
            .payload(Receipt::deserialize_from(payload)?)
            .map_err(queue_error)?;
        let delay = Delay::For(Duration::from_secs(u64::from(seconds)));
        store
            .defer_processing(
                call,
                token,
                seconds,
                policy,
                route_snapshot,
                move |transaction| {
                    Box::pin(async move {
                        route
                            .enqueue(&manager, next, Some(delay), transaction)
                            .await
                    })
                },
            )
            .await
            .map_err(queue_error)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueRoute {
    connection: String,
    queue: String,
    job: String,
    options: JobOptions,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    recover_deferred: bool,
}

impl QueueRoute {
    fn encode(&self) -> Result<String> {
        let value = serde_json::to_string(self).map_err(|_| Error::InvalidRecord)?;
        if value.len() > 16 * 1024 {
            return Err(Error::InvalidRecord);
        }
        Ok(value)
    }

    fn dispatch_encoding(mut self) -> Result<String> {
        self.recover_deferred = false;
        self.encode()
    }

    fn decode(value: &str) -> Result<Self> {
        if value.len() > 16 * 1024 {
            return Err(Error::InvalidRecord);
        }
        serde_json::from_str(value).map_err(|_| Error::InvalidRecord)
    }

    fn payload(&self, receipt: Receipt<'_>) -> Result<Payload> {
        Ok(Payload::new(job_name(&self.job)?, receipt)?.with_options(self.options.clone()))
    }

    async fn enqueue(
        self,
        queue: &QueueManager,
        payload: Payload,
        delay: Option<Delay>,
        transaction: &mut openrust::database::Transaction,
    ) -> Result<()> {
        if !queue.has_handler(payload.name()) {
            return Err(QueueError::UnknownJob(payload.name().to_owned()).into());
        }
        let options = DispatchOptions {
            connection: Some(self.connection),
            queue: Some(self.queue),
            delay,
            ..Default::default()
        };
        match queue
            .dispatch_in_database_transaction(payload, options, transaction)
            .await?
        {
            DispatchResult::Queued(_) => Ok(()),
            _ => Err(QueueError::InvalidConfiguration.into()),
        }
    }
}

impl<'a> Receipt<'a> {
    fn deserialize_from(payload: &'a Payload) -> QueueResult<Self> {
        let receipt = <Self as serde::Deserialize>::deserialize(payload.data())
            .map_err(|_| QueueError::InvalidPayload)?;
        receipt.validate()?;
        Ok(receipt)
    }

    fn validate(&self) -> QueueResult<()> {
        if [self.id, self.scope, self.profile, self.revision]
            .iter()
            .any(|value| {
                value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
            })
        {
            return Err(QueueError::InvalidPayload);
        }
        Ok(())
    }
}

pub(crate) fn queue_error(source: Error) -> QueueError {
    QueueError::Execution {
        source: Arc::new(source),
    }
}
