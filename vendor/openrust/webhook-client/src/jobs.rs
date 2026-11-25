//! 通过原生队列在独立 Worker 中重新读取接收记录。
use crate::{
    DatabaseWebhookStore,
    storage::{ProcessingClaim, ProcessingOutcome},
};
use crate::{Error, Result, WebhookJobDispatcher, WebhookStore, models::WebhookCall};
use openrust::{
    bus::Bus,
    contracts::queue::JobHandler,
    http::HttpFuture,
    queue::{
        DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError, QueueFacade,
        QueueFuture, QueueManager, QueueResult,
    },
};
use std::{sync::Arc, time::Duration};

/// 应用业务处理器从其注册的存储读取载荷及附件。
pub trait ProcessWebhookJob: Send + Sync {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        call: &'a WebhookCall,
        store: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, Result<()>>;
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
        if !(3..=86_400).contains(&lease_seconds) {
            return Err(Error::InvalidConfig("processing_lease_seconds"));
        }
        queue.register(
            &job_name(name)?,
            Arc::new(RegisteredJob {
                store: store.clone(),
                handler,
                lease: Some((store, lease_seconds)),
            }),
        )?;
        Ok(())
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

fn job_name(name: &str) -> Result<String> {
    crate::factories::validate_reference("process_webhook_job", name)?;
    Ok(format!("openrust_webhook_client::jobs::{name}"))
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
            ProcessingClaim::Complete => return Ok(()),
            ProcessingClaim::Busy => {
                context.release(Duration::from_secs(u64::from(seconds)));
                return Ok(());
            }
        };
        let mut call = call;
        call.exception = None;
        let failed = self.handler.handle(context, &call, store).await.is_err();
        let outcome = if failed {
            ProcessingOutcome::Failed
        } else if context.release_delay().is_some() {
            ProcessingOutcome::Release
        } else {
            ProcessingOutcome::Complete
        };
        store
            .finish_processing(&call, &token, outcome)
            .await
            .map_err(queue_error)?;
        if failed {
            return Err(queue_error(Error::ProcessingFailed));
        }
        Ok(())
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

fn queue_error(source: Error) -> QueueError {
    QueueError::Execution {
        source: Arc::new(source),
    }
}
