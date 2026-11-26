use super::agent::{Agent, Message};
use crate::{
    contracts::queue::Job,
    queue::{
        Delay, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueResult,
        sqs_queue::SqsQueue,
    },
};
use std::{sync::Arc, time::Duration};
/// 先记录终态再上报；确认丢失时不能再把已处理任务作为未处理任务释放。
pub(crate) struct AgentJob {
    inner: Box<dyn Job>,
    queue: Arc<SqsQueue>,
    agent: Agent,
    message: Message,
    deleted: bool,
    released: bool,
}
impl AgentJob {
    pub async fn new(queue: Arc<SqsQueue>, agent: Agent, message: Message) -> QueueResult<Self> {
        let inner = queue
            .agent_job(
                message.id.clone(),
                &message.body,
                message.queue_url.clone().unwrap_or_default(),
                message.attempts,
            )
            .await?;
        Ok(Self {
            inner,
            queue,
            agent,
            message,
            deleted: false,
            released: false,
        })
    }
    async fn processed(&mut self) -> QueueResult<()> {
        self.deleted = true;
        self.agent
            .report(
                &self.message.id,
                self.message.receipt.as_deref(),
                "processed",
                None,
            )
            .await?;
        self.queue.delete_agent_overflow(&self.message.body).await
    }
}
impl Job for AgentJob {
    fn uuid(&self) -> &str {
        self.inner.uuid()
    }
    fn get_job_id(&self) -> &str {
        self.inner.get_job_id()
    }
    fn get_connection_name(&self) -> &str {
        self.inner.get_connection_name()
    }
    fn get_queue(&self) -> &str {
        self.inner.get_queue()
    }
    fn payload(&self) -> &Payload {
        self.inner.payload()
    }
    fn options(&self) -> &JobOptions {
        self.inner.options()
    }
    fn attempts(&self) -> u32 {
        self.inner.attempts()
    }
    fn is_deleted(&self) -> bool {
        self.deleted
    }
    fn is_released(&self) -> bool {
        self.released
    }
    fn has_failed(&self) -> bool {
        self.inner.has_failed()
    }
    fn mark_as_failed(&mut self) {
        self.inner.mark_as_failed()
    }
    fn supports_isolated_execution(&self) -> bool {
        self.inner.supports_isolated_execution()
    }
    fn fire<'a>(&'a mut self, context: &'a mut JobContext<'_>) -> QueueFuture<'a, QueueResult<()>> {
        self.inner.fire(context)
    }
    fn fail<'a>(&'a mut self, _cause: Option<&'a QueueError>) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.inner.mark_as_failed();
            self.processed().await
        })
    }
    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            self.released = true;
            self.agent
                .report(
                    &self.message.id,
                    self.message.receipt.as_deref(),
                    "released",
                    Some(delay.as_secs()),
                )
                .await
        })
    }
    fn advance_chain<'a>(&'a mut self, next: &'a Payload) -> QueueFuture<'a, QueueResult<()>> {
        self.advance_chain_to(next, None, None)
    }
    fn advance_chain_to<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.inner.advance_chain_to(next, queue, delay).await?;
            self.processed().await
        })
    }
    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(self.processed())
    }
}
