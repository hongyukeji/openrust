use super::{ProcessWebhookJob, QueueRoute, QueuedWebhookJob, Receipt};
use crate::{DatabaseWebhookStore, Error, Result, storage::text};
use openrust::queue::{
    DispatchOptions, DispatchResult, Payload, QueueError, QueueFacade, QueueManager,
};
use std::{sync::Arc, time::Duration};

const RECOVERY_JOB_PREFIX: &str = "openrust_webhook_client::dispatch_recovery::";
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(15);

/// 可持久化的具名队列路由。只保存连接、队列、Job 注册名及选项。
pub struct RecoveryDispatch(QueueRoute);

/// 一次有界扫描的候选、已派发和待重试数量。
#[derive(Default)]
pub struct DispatchRecoveryReport {
    pub candidates: usize,
    pub dispatched: usize,
    pub failed: usize,
}

impl QueuedWebhookJob {
    /// 注册可恢复派发的数据库租约处理器，接收节点、恢复进程与 Worker 使用同名注册。
    pub fn register_with_database_recovery(
        queue: &QueueManager,
        name: &str,
        store: Arc<DatabaseWebhookStore>,
        handler: Arc<dyn ProcessWebhookJob>,
        lease_seconds: u32,
    ) -> Result<()> {
        Self::register_leased(
            queue,
            recovery_job_name(name)?,
            store,
            handler,
            lease_seconds,
            None,
        )
    }

    pub(super) fn snapshot_recovery_route(&self) -> Result<RecoveryDispatch> {
        let connection = self
            .dispatch
            .connection
            .as_deref()
            .ok_or(Error::InvalidConfig("recover_dispatch.connection"))?;
        let queue = self
            .dispatch
            .queue
            .as_deref()
            .ok_or(Error::InvalidConfig("recover_dispatch.queue"))?;
        let job = self
            .name
            .strip_prefix(super::JOB_PREFIX)
            .ok_or(Error::InvalidRecord)?;
        let route = RecoveryDispatch(QueueRoute {
            connection: connection.into(),
            queue: queue.into(),
            job: job.into(),
            options: self.options.clone(),
        });
        let manager = QueueFacade::manager()?;
        route.validate(&manager)?;
        Ok(route)
    }

    /// 扫描后通过原生队列派发。在队列确认前保留意图，超时或进程退出后可以重试。
    pub async fn recover_dispatches(
        store: &DatabaseWebhookStore,
        queue: &QueueManager,
        limit: u32,
        pretend: bool,
    ) -> Result<DispatchRecoveryReport> {
        let rows = store.due_dispatch_intents(limit).await?;
        let mut report = DispatchRecoveryReport {
            candidates: rows.len(),
            ..Default::default()
        };
        if pretend {
            return Ok(report);
        }
        for row in rows {
            let id = text(&row, "call_id")?;
            let Some(token) = store.claim_dispatch(id).await? else {
                continue;
            };
            let Some((scope, profile, revision)) = store.dispatch_metadata(id).await? else {
                continue;
            };
            let receipt = Receipt {
                id,
                scope: &scope,
                profile: &profile,
                revision: &revision,
            };
            let queued = async {
                let route = RecoveryDispatch(QueueRoute::decode(text(&row, "route")?)?);
                route.enqueue(queue, receipt).await
            }
            .await;
            if queued.is_err() {
                report.failed += 1;
                continue;
            }
            report.dispatched += usize::from(store.finish_dispatch(id, &token).await?);
        }
        Ok(report)
    }
}

impl RecoveryDispatch {
    pub(crate) fn encode(&self) -> Result<String> {
        self.0.encode()
    }

    fn validate(&self, queue: &QueueManager) -> Result<String> {
        let route = &self.0;
        for (setting, name) in [
            ("recover_dispatch.connection", &route.connection),
            ("recover_dispatch.queue", &route.queue),
        ] {
            if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
                return Err(Error::InvalidConfig(setting));
            }
        }
        if route
            .options
            .timeout
            .is_none_or(|timeout| timeout.is_zero())
        {
            return Err(Error::InvalidConfig("recover_dispatch.timeout"));
        }
        let name = recovery_job_name(&route.job)?;
        if !queue.has_handler(&name) {
            return Err(QueueError::UnknownJob(name).into());
        }
        if !queue.connection_is_direct_worker_queue(&route.connection)? {
            return Err(Error::InvalidConfig("recover_dispatch.connection"));
        }
        Ok(name)
    }

    async fn enqueue(self, queue: &QueueManager, receipt: Receipt<'_>) -> Result<()> {
        let name = self.validate(queue)?;
        receipt.validate()?;
        let route = self.0;
        let payload = Payload::new(&name, receipt)?.with_options(route.options);
        let options = DispatchOptions {
            connection: Some(route.connection),
            queue: Some(route.queue),
            after_commit: Some(false),
            ..Default::default()
        };
        let sent = tokio::time::timeout(DISPATCH_TIMEOUT, queue.dispatch(payload, options, None))
            .await
            .map_err(|_| Error::DispatchFailed)??;
        match sent {
            DispatchResult::Queued(_) => Ok(()),
            _ => Err(Error::DispatchFailed),
        }
    }
}

fn recovery_job_name(name: &str) -> Result<String> {
    crate::factories::validate_reference("process_webhook_job", name)?;
    Ok(format!("{RECOVERY_JOB_PREFIX}{name}"))
}
