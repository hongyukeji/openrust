use std::{marker::PhantomData, sync::Arc};

use openrust::{
    contracts::queue::JobHandler,
    queue::{
        Delay, DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError,
        QueueFuture, QueueManager, QueueResult,
    },
    support::facades::Queue,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Agent, AgentResponse, AiError, PromptOptions};

pub type AgentQueueThen<A> =
    for<'a> fn(&'a A, &'a AgentResponse) -> QueueFuture<'a, QueueResult<()>>;
pub type AgentQueueCatch<A> = for<'a> fn(&'a A, &'a QueueError) -> QueueFuture<'a, QueueResult<()>>;

/// 可由独立 Worker 恢复的 Agent。任务名在应用与 Worker 之间保持稳定。
pub trait QueueableAgent: Agent + Serialize + DeserializeOwned + Sized {
    const QUEUE_JOB: &'static str;

    fn register_queue(queue: &QueueManager) -> QueueResult<()> {
        queue.register(Self::QUEUE_JOB, Arc::new(InvokeAgent::<Self>(PhantomData)))
    }

    fn queued_then(_name: &str) -> Option<AgentQueueThen<Self>> {
        None
    }

    fn queued_catch(_name: &str) -> Option<AgentQueueCatch<Self>> {
        None
    }
}

#[derive(Serialize, Deserialize)]
struct Invocation<A> {
    agent: A,
    text: String,
    provider: Option<String>,
    model: Option<String>,
    timeout: Option<std::time::Duration>,
    then: Vec<String>,
    catch: Vec<String>,
}

impl<A> Invocation<A> {
    fn prompt_options(&self) -> PromptOptions<'_> {
        PromptOptions {
            provider: self.provider.as_deref(),
            model: self.model.as_deref(),
            timeout: self.timeout,
        }
    }
}

/// 调用 `dispatch` 前可设置队列、重试与已注册的完成回调。
pub struct QueuedAgentResponse<'a, A> {
    invocation: Invocation<&'a A>,
    dispatch: DispatchOptions,
    job: JobOptions,
}

impl<'a, A: QueueableAgent> QueuedAgentResponse<'a, A> {
    pub(crate) fn new(agent: &'a A, prompt: impl Into<String>, options: PromptOptions<'_>) -> Self {
        Self {
            invocation: Invocation {
                agent,
                text: prompt.into(),
                provider: options.provider.map(str::to_owned),
                model: options.model.map(str::to_owned),
                timeout: options.timeout,
                then: Vec::new(),
                catch: Vec::new(),
            },
            dispatch: DispatchOptions::default(),
            job: JobOptions::default(),
        }
    }

    pub fn on_connection(mut self, name: impl Into<String>) -> Self {
        self.dispatch.connection = Some(name.into());
        self
    }

    pub fn on_queue(mut self, name: impl Into<String>) -> Self {
        self.dispatch.queue = Some(name.into());
        self
    }

    pub fn delay(mut self, delay: impl Into<Delay>) -> Self {
        self.dispatch.delay = Some(delay.into());
        self
    }

    pub fn with_job_options(mut self, options: JobOptions) -> Self {
        self.job = options;
        self
    }

    pub fn then(mut self, name: impl Into<String>) -> Self {
        self.invocation.then.push(name.into());
        self
    }

    pub fn catch(mut self, name: impl Into<String>) -> Self {
        self.invocation.catch.push(name.into());
        self
    }

    pub async fn dispatch(self) -> QueueResult<DispatchResult> {
        if self.invocation.text.trim().is_empty() {
            return Err(QueueError::execution(AiError::InvalidPrompt));
        }
        validate_callbacks::<A>(&self.invocation.then, &self.invocation.catch)?;
        let payload = Payload::new(A::QUEUE_JOB, &self.invocation)?.with_options(self.job);
        Queue::dispatch(payload, self.dispatch, None).await
    }
}

fn validate_callbacks<A: QueueableAgent>(then: &[String], catch: &[String]) -> QueueResult<()> {
    if then.iter().any(|name| A::queued_then(name).is_none())
        || catch.iter().any(|name| A::queued_catch(name).is_none())
    {
        return Err(QueueError::InvalidConfiguration);
    }
    Ok(())
}

struct InvokeAgent<A>(PhantomData<A>);

impl<A: QueueableAgent> JobHandler for InvokeAgent<A> {
    fn queue_subject_type(&self) -> &'static str {
        std::any::type_name::<A>()
    }

    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let work: Invocation<A> = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            validate_callbacks::<A>(&work.then, &work.catch)?;
            let response = work
                .agent
                .prompt_with(&work.text, work.prompt_options())
                .await
                .map_err(QueueError::execution)?;
            for name in &work.then {
                let then = A::queued_then(name).ok_or(QueueError::InvalidConfiguration)?;
                then(&work.agent, &response).await?;
            }
            Ok(())
        })
    }

    fn failed<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
        error: &'a QueueError,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let work: Invocation<A> = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            // 终态即便其他回调失败，也执行其余已明确注册的失败回调。
            let mut failure = None;
            for name in &work.catch {
                let result = match A::queued_catch(name) {
                    Some(catch) => catch(&work.agent, error).await,
                    None => Err(QueueError::InvalidConfiguration),
                };
                if let Err(error) = result {
                    failure.get_or_insert(error);
                }
            }
            failure.map_or(Ok(()), Err)
        })
    }
}
