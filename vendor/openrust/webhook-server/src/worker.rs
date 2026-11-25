use std::{sync::Arc, time::Duration};

use openrust::{
    contracts::queue::JobHandler,
    queue::{
        DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError, QueueFuture,
        QueueManager, QueueResult,
    },
    support::facades::Config,
};

use crate::{DeliveryProgress, DeliveryService, Error, Result, extensions};

impl DeliveryService {
    /// 注册指定范围的发送任务，Worker 和应用应使用同一份策略配置。
    pub fn register_worker(&self, queue: &QueueManager, name: &str) -> Result<()> {
        queue.register(
            &self.worker_name(name)?,
            Arc::new(DeliveryJob(self.clone())),
        )?;
        Ok(())
    }

    /// 已持久提交的投递可加入具名 Worker 队列。队列仅收到内部投递 ID。
    pub async fn dispatch(&self, queue: &QueueManager, id: &str) -> Result<DispatchResult> {
        let snapshot = self.store.snapshot(id).await?.ok_or(Error::InvalidRecord)?;
        let name = self.worker_name(&snapshot.job)?;
        if !queue.has_handler(&name) {
            return Err(Error::InvalidConfig("delivery.job"));
        }
        let connection = snapshot
            .connection
            .or_else(|| {
                Config::get("queue.default")
                    .ok()
                    .flatten()?
                    .as_str()
                    .map(str::to_owned)
            })
            .ok_or(Error::InvalidConfig("delivery.connection"))?;
        if !queue.connection_is_direct_worker_queue(&connection)? {
            return Err(Error::InvalidConfig("delivery.connection"));
        }
        let payload = Payload::new(name, id)?.with_options(JobOptions {
            // 重试由投递记录控制，底层任务不叠加额度。
            max_tries: Some(1),
            timeout: Some(Duration::from_secs(u64::from(snapshot.timeout) + 3)),
            ..Default::default()
        });
        Ok(queue
            .dispatch(
                payload,
                DispatchOptions {
                    connection: Some(connection),
                    queue: snapshot.queue,
                    after_commit: Some(false),
                    ..Default::default()
                },
                None,
            )
            .await?)
    }

    fn worker_name(&self, name: &str) -> Result<String> {
        extensions::validate_name("delivery.job", name)?;
        Ok(format!(
            "openrust_webhook_server.{}.{name}",
            self.store.worker_key()
        ))
    }
}

struct DeliveryJob(DeliveryService);

impl JobHandler for DeliveryJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let id = payload.data().as_str().ok_or(QueueError::InvalidPayload)?;
            let (progress, throw) = self.0.run(id).await.map_err(queue_error)?;
            if throw
                && matches!(
                    progress,
                    DeliveryProgress::Failed | DeliveryProgress::Expired
                )
            {
                return Err(queue_error(Error::DeliveryFailed));
            }
            Ok(())
        })
    }
}

fn queue_error(source: Error) -> QueueError {
    QueueError::Execution {
        source: Arc::new(source),
    }
}
