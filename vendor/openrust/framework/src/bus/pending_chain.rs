use std::collections::{HashSet, VecDeque};

use crate::{
    queue::{
        Delay, DispatchOptions, DispatchResult, Payload, QueueError, QueueFuture, QueueResult,
    },
    support::facades::Queue,
};

use super::{ChainedBatch, PendingBatch};

/// An application callback invoked when a chain member fails permanently.
pub type ChainCallback =
    for<'a> fn(&'a Payload, &'a QueueError) -> QueueFuture<'a, QueueResult<()>>;

/// A boolean or a lazy test for conditional chain dispatch.
pub trait ChainCondition {
    fn evaluate(self) -> bool;
}

impl ChainCondition for bool {
    fn evaluate(self) -> bool {
        self
    }
}

impl<F: FnOnce() -> bool> ChainCondition for F {
    fn evaluate(self) -> bool {
        self()
    }
}

/// Dispatches jobs in sequence, with each success making the next job available.
pub struct PendingChain {
    jobs: VecDeque<Payload>,
    options: DispatchOptions,
    catches: Vec<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel PendingChain")]
impl PendingChain {
    pub(super) fn new(jobs: impl IntoIterator<Item = Payload>) -> Self {
        Self {
            jobs: jobs.into_iter().collect(),
            options: DispatchOptions::default(),
            catches: Vec::new(),
        }
    }

    pub fn onConnection(mut self, name: impl Into<String>) -> Self {
        self.options.connection = Some(name.into());
        self
    }

    pub fn onQueue(mut self, name: impl Into<String>) -> Self {
        self.options.queue = Some(name.into());
        self
    }

    /// Delays the first job in the chain.
    pub fn delay(mut self, delay: Delay) -> Self {
        self.options.delay = Some(delay);
        self
    }

    pub fn prepend(mut self, job: Payload) -> Self {
        self.jobs.push_front(job);
        self
    }

    pub fn append(mut self, job: Payload) -> Self {
        self.jobs.push_back(job);
        self
    }

    /// Adds a batch as one step, waiting for its terminal state before continuing.
    pub fn appendBatch(self, batch: PendingBatch) -> QueueResult<Self> {
        Ok(self.append(ChainedBatch::payload(batch)?))
    }

    pub fn prependBatch(self, batch: PendingBatch) -> QueueResult<Self> {
        Ok(self.prepend(ChainedBatch::payload(batch)?))
    }

    pub fn catch(mut self, callback: impl Into<String>) -> Self {
        self.catches.push(callback.into());
        self
    }

    /// Dispatches the chain if the condition passes.
    pub async fn dispatchIf(
        self,
        condition: impl ChainCondition,
    ) -> QueueResult<Option<DispatchResult>> {
        if condition.evaluate() {
            return self.dispatch().await.map(Some);
        }
        Ok(None)
    }

    /// Dispatches the chain if the condition does not pass.
    pub async fn dispatchUnless(
        self,
        condition: impl ChainCondition,
    ) -> QueueResult<Option<DispatchResult>> {
        self.dispatchIf(!condition.evaluate()).await
    }

    pub async fn dispatch(mut self) -> QueueResult<DispatchResult> {
        let mut ids = HashSet::with_capacity(self.jobs.len());
        if self
            .jobs
            .iter()
            .any(|job| !job.is_unattached() || !ids.insert(job.uuid()))
        {
            return Err(QueueError::InvalidPayload);
        }
        let queue = Queue::manager()?;
        queue.validate_chained_batches(&self.jobs, &self.options)?;
        let head = self.jobs.pop_front().ok_or(QueueError::InvalidPayload)?;
        for name in &self.catches {
            queue.chain_callback(name)?;
        }
        queue
            .dispatch(head.with_chain(self.jobs, self.catches), self.options, None)
            .await
    }
}
