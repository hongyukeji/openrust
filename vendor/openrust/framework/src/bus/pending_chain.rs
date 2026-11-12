use std::collections::{HashSet, VecDeque};

use crate::{
    queue::{
        Delay, DispatchOptions, DispatchResult, Payload, QueueError, QueueFuture, QueueResult,
    },
    support::facades::Queue,
};

/// An application callback invoked when a chain member fails permanently.
pub type ChainCallback =
    for<'a> fn(&'a Payload, &'a QueueError) -> QueueFuture<'a, QueueResult<()>>;

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

    pub fn catch(mut self, callback: impl Into<String>) -> Self {
        self.catches.push(callback.into());
        self
    }

    pub async fn dispatch(mut self) -> QueueResult<DispatchResult> {
        let mut ids = HashSet::with_capacity(self.jobs.len());
        if self
            .jobs
            .iter()
            .any(|job| !job.is_chain_member() || !ids.insert(job.uuid()))
        {
            return Err(QueueError::InvalidPayload);
        }
        let head = self.jobs.pop_front().ok_or(QueueError::InvalidPayload)?;
        let queue = Queue::manager()?;
        for name in &self.catches {
            queue.chain_callback(name)?;
        }
        queue
            .dispatch(head.with_chain(self.jobs, self.catches), self.options, None)
            .await
    }
}
