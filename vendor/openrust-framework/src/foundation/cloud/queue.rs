use crate::{
    contracts::queue::{ClearableQueue, Job, Queue as Contract, QueueMetrics},
    queue::{Delay, Payload, QueueError, QueueFuture, QueueResult, sqs_queue::SqsQueue},
};
use serde_json::Value;
use std::{
    sync::{Arc, PoisonError, RwLock},
    time::Duration,
};
/// 管理器与公开门面共用同一连接；运行期名称及元数据在锁外替换快照。
pub struct Queue {
    inner: RwLock<Arc<SqsQueue>>,
    config: RwLock<Value>,
    default_queue: String,
    pub(super) owner: std::sync::Weak<crate::container::Container>,
    pub(super) identity: Arc<()>,
    pub(super) execution: Arc<super::telemetry::Execution>,
}
impl Queue {
    pub(crate) fn new(
        inner: Arc<SqsQueue>,
        config: Value,
        owner: std::sync::Weak<crate::container::Container>,
    ) -> Self {
        Self {
            identity: Arc::new(()),
            execution: Arc::default(),
            owner,
            default_queue: inner.default_queue().to_owned(),
            inner: RwLock::new(inner),
            config: RwLock::new(config),
        }
    }
    fn target(&self) -> Arc<SqsQueue> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    fn settings(&self) -> Value {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn set_connection_name(&self, name: impl Into<String>) -> &Self {
        let target = Arc::new(self.target().renamed(name.into()));
        let retired = std::mem::replace(
            &mut *self.inner.write().unwrap_or_else(PoisonError::into_inner),
            target,
        );
        drop(retired);
        self
    }
    pub fn get_connection_name(&self) -> String {
        self.target().get_connection_name()
    }
    pub fn set_config(&self, config: Value) -> QueueResult<&Self> {
        if !config.get("connection").is_some_and(Value::is_object) {
            return Err(QueueError::InvalidConfiguration);
        }
        let retired = std::mem::replace(
            &mut *self.config.write().unwrap_or_else(PoisonError::into_inner),
            config,
        );
        drop(retired);
        Ok(self)
    }
    pub fn get_config(&self) -> Value {
        self.settings()
            .get("connection")
            .cloned()
            .unwrap_or(Value::Null)
    }
    pub async fn get_sqs(&self) -> aws_sdk_sqs::Client {
        self.target().client().await.clone()
    }
    pub fn get_queue(&self, queue: Option<&str>) -> QueueResult<String> {
        self.target().url(queue)
    }
    pub fn managed_queues(&self) -> QueueResult<Vec<Value>> {
        match self.settings().get("queues") {
            None | Some(Value::Null) => Ok(vec![]),
            Some(Value::Array(queues)) => Ok(queues.clone()),
            Some(Value::Object(queues)) => {
                let keys = queues.keys().map(|key| php_key(key)).collect::<Vec<_>>();
                if keys
                    .iter()
                    .enumerate()
                    .all(|(index, key)| key.as_u64() == Some(index as u64))
                {
                    Ok(queues.values().cloned().collect())
                } else {
                    Ok(keys)
                }
            }
            _ => Err(QueueError::InvalidConfiguration),
        }
    }
    pub fn normalize_queue(&self, queue: Option<&str>) -> QueueResult<String> {
        let mut queue = self.target().url(queue)?;
        let settings = self.settings();
        let connection = settings.get("connection");
        if let Some(prefix) = connection
            .and_then(|value| value.get("prefix"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && *value != "0")
            && let Some(name) = queue.strip_prefix(&format!("{prefix}/"))
        {
            queue = name.to_owned()
        }
        if let Some(suffix) = connection
            .and_then(|value| value.get("suffix"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && *value != "0")
        {
            if let Some(name) = queue.strip_suffix(".fifo") {
                queue = format!("{}.fifo", name.strip_suffix(suffix).unwrap_or(name));
            } else if let Some(name) = queue.strip_suffix(suffix) {
                queue = name.to_owned()
            }
        }
        Ok(queue)
    }
    pub async fn total_size(&self) -> QueueResult<u64> {
        self.total(0).await
    }
    pub async fn total_pending_size(&self) -> QueueResult<u64> {
        self.total(1).await
    }
    pub async fn total_delayed_size(&self) -> QueueResult<u64> {
        self.total(2).await
    }
    pub async fn total_reserved_size(&self) -> QueueResult<u64> {
        self.total(3).await
    }
    async fn total(&self, kind: u8) -> QueueResult<u64> {
        let mut count = 0u64;
        for name in self.managed_queues()? {
            let name = queue_name(&name)?;
            let size = match kind {
                0 => self.size(name.as_deref()).await?,
                1 => self.pending_size(name.as_deref()).await?.unwrap_or(0),
                2 => self.delayed_size(name.as_deref()).await?.unwrap_or(0),
                _ => self.reserved_size(name.as_deref()).await?.unwrap_or(0),
            };
            count = count
                .checked_add(size)
                .ok_or(QueueError::InvalidConfiguration)?;
        }
        Ok(count)
    }
    async fn agent_pop(&self, queue: Option<&str>) -> QueueResult<Option<Option<Box<dyn Job>>>> {
        use crate::contracts::container::Container as _;
        let config = self.settings();
        if config.get("agent").and_then(|a| a.get("enabled")) != Some(&Value::Bool(true)) {
            return Ok(None);
        }
        let Some(selection) = super::worker::selection(self) else {
            return Ok(None);
        };
        if self.get_queue(queue)? != self.get_queue(Some(&selection))? {
            return Ok(None);
        }
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let socket = config
            .get("agent")
            .and_then(|a| a.get("socket"))
            .and_then(Value::as_str)
            .unwrap_or("/tmp/cloud-agent.sock");
        let agent =
            super::agent::Agent::new(owner.make::<crate::http::client::Factory>()?, socket.into());
        let job = match agent.next().await? {
            Some(message) => Some(Box::new(
                super::job::AgentJob::new(self.target(), agent, message).await?,
            ) as Box<dyn Job>),
            None => None,
        };
        Ok(Some(job))
    }
    pub async fn size(&self, queue: Option<&str>) -> QueueResult<u64> {
        self.target().size(queue).await
    }
    pub async fn metrics(&self, queue: Option<&str>) -> QueueResult<QueueMetrics> {
        self.target().metrics(queue).await
    }
    pub async fn pending_size(&self, queue: Option<&str>) -> QueueResult<Option<u64>> {
        self.target().pending_size(queue).await
    }
    pub async fn delayed_size(&self, queue: Option<&str>) -> QueueResult<Option<u64>> {
        self.target().delayed_size(queue).await
    }
    pub async fn reserved_size(&self, queue: Option<&str>) -> QueueResult<Option<u64>> {
        self.target().reserved_size(queue).await
    }
    pub async fn creation_time_of_oldest_pending_job(
        &self,
        queue: Option<&str>,
    ) -> QueueResult<Option<u64>> {
        self.target()
            .creation_time_of_oldest_pending_job(queue)
            .await
    }
    pub async fn push(
        &self,
        payload: &Payload,
        queue: Option<&str>,
    ) -> QueueResult<Option<String>> {
        let result = self.target().push(payload, queue).await?;
        self.finish_queueing_job(queue).await?;
        Ok(result)
    }
    pub async fn later(
        &self,
        delay: Delay,
        payload: &Payload,
        queue: Option<&str>,
    ) -> QueueResult<Option<String>> {
        let result = self.target().later(delay, payload, queue).await?;
        self.finish_queueing_job(queue).await?;
        Ok(result)
    }
    pub async fn push_on(&self, queue: &str, payload: &Payload) -> QueueResult<Option<String>> {
        self.push(payload, Some(queue)).await
    }
    pub async fn later_on(
        &self,
        queue: &str,
        delay: Delay,
        payload: &Payload,
    ) -> QueueResult<Option<String>> {
        self.later(delay, payload, Some(queue)).await
    }
    pub async fn pop(&self, queue: Option<&str>) -> QueueResult<Option<Box<dyn Job>>> {
        self.finish_processing_job(None, None).await?;
        let job = if let Some(job) = self.agent_pop(queue).await? {
            job
        } else {
            self.target().pop(queue).await?
        };
        self.start_processing_job(queue, job).await
    }
    pub async fn pop_now(&self, queue: Option<&str>) -> QueueResult<Option<Box<dyn Job>>> {
        self.finish_processing_job(None, None).await?;
        let job = if let Some(job) = self.agent_pop(queue).await? {
            job
        } else {
            self.target().pop_now(queue).await?
        };
        self.start_processing_job(queue, job).await
    }
    pub async fn pop_within(
        &self,
        queue: Option<&str>,
        wait: Duration,
    ) -> QueueResult<Option<Box<dyn Job>>> {
        self.finish_processing_job(None, None).await?;
        let job = if let Some(job) = self.agent_pop(queue).await? {
            job
        } else {
            self.target().pop_within(queue, wait).await?
        };
        self.start_processing_job(queue, job).await
    }
    pub async fn clear(&self, queue: &str) -> QueueResult<u64> {
        self.target().clear(queue).await
    }
}
fn php_key(key: &str) -> Value {
    match key.parse::<i64>() {
        Ok(number) if number.to_string() == key => number.into(),
        _ => key.into(),
    }
}
fn queue_name(value: &Value) -> QueueResult<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.clone())),
        Value::Number(value) => Ok(Some(value.to_string())),
        Value::Bool(value) => Ok(Some(if *value { "1" } else { "" }.into())),
        _ => Err(QueueError::InvalidConfiguration),
    }
}
impl Contract for Queue {
    fn get_connection_name(&self) -> String {
        self.get_connection_name()
    }
    fn into_any(self: Arc<Self>) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        Some(self)
    }
    fn default_queue(&self) -> &str {
        &self.default_queue
    }
    fn clearable(&self) -> Option<&dyn ClearableQueue> {
        Some(self)
    }
    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(self.size(queue))
    }
    fn metrics<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(self.metrics(queue))
    }
    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.push(payload, queue))
    }
    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.later(delay, payload, queue))
    }
    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.pop(queue))
    }
    fn pop_now<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.pop_now(queue))
    }
    fn pop_within<'a>(
        &'a self,
        queue: Option<&'a str>,
        wait: Duration,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(self.pop_within(queue, wait))
    }
}
impl ClearableQueue for Queue {
    fn clear<'a>(&'a self, queue: &'a str) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(self.clear(queue))
    }
}
