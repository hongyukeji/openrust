use std::{sync::Arc, time::Duration};

use openrust::{
    contracts::queue::JobHandler,
    queue::{
        DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError, QueueFuture,
        QueueManager, QueueResult,
    },
    support::facades::Config,
};

use crate::{DeliveryService, Error, Result, events, extensions};

impl DeliveryService {
    /// 注册指定范围的发送任务，Worker 和应用应使用同一份策略配置。
    pub fn register_worker(&self, queue: &QueueManager, name: &str) -> Result<()> {
        queue.register(
            &self.worker_name(name)?,
            Arc::new(DeliveryJob {
                service: self.clone(),
                name: name.into(),
            }),
        )?;
        Ok(())
    }

    /// 已持久提交的投递可加入具名 Worker 队列。队列仅收到内部投递 ID。
    pub async fn dispatch(&self, queue: &QueueManager, id: &str) -> Result<DispatchResult> {
        self.enqueue_delivery(queue, id, true).await
    }

    pub(crate) async fn enqueue_delivery(
        &self,
        queue: &QueueManager,
        id: &str,
        explicit: bool,
    ) -> Result<DispatchResult> {
        let snapshot = self.store.snapshot(id).await?.ok_or(Error::InvalidRecord)?;
        if snapshot.custom_job {
            extensions::call_webhook_job(&snapshot.job)?;
        }
        let (name, connection) =
            self.queue_route(queue, &snapshot.job, snapshot.connection.as_deref())?;
        let selected_queue = snapshot.queue.clone();
        let payload = Payload::new(name, id)?.with_options(JobOptions {
            // 重试由投递记录控制，底层任务不叠加额度。
            max_tries: Some(1),
            timeout: Some(Duration::from_secs(u64::from(snapshot.timeout) + 3)),
            ..Default::default()
        });
        if explicit {
            self.store.defer_recovery(id).await?;
        }
        events::dispatching(self.store.scope(), id, snapshot).await?;
        Ok(queue
            .dispatch(
                payload,
                DispatchOptions {
                    connection: Some(connection),
                    queue: selected_queue,
                    after_commit: Some(false),
                    ..Default::default()
                },
                None,
            )
            .await?)
    }

    pub(crate) fn queue_route(
        &self,
        queue: &QueueManager,
        job: &str,
        connection: Option<&str>,
    ) -> Result<(String, String)> {
        let name = self.worker_name(job)?;
        if !queue.has_handler(&name) {
            return Err(Error::InvalidConfig("delivery.job"));
        }
        let connection = connection
            .map(str::to_owned)
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
        Ok((name, connection))
    }

    fn worker_name(&self, name: &str) -> Result<String> {
        extensions::validate_name("delivery.job", name)?;
        Ok(format!(
            "openrust_webhook_server.{}.{name}",
            self.store.worker_key()
        ))
    }
}

struct DeliveryJob {
    service: DeliveryService,
    name: String,
}

impl JobHandler for DeliveryJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let id = payload.data().as_str().ok_or(QueueError::InvalidPayload)?;
            let (progress, throw) = self
                .service
                .run(id, Some(&self.name))
                .await
                .map_err(queue_error)?;
            if throw && progress.is_final_failure() {
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
