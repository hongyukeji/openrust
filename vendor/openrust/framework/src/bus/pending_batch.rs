use std::collections::VecDeque;

use serde_json::{Map, Value};

use crate::{
    queue::{Payload, QueueResult},
    support::facades::Queue,
};

use super::{Batch, BatchPhase};

#[allow(
    clippy::large_enum_variant,
    reason = "Keeps ordinary jobs inline in the batch builder"
)]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum BatchJob {
    Job(Payload),
    Chain(VecDeque<Payload>),
}

impl BatchJob {
    pub(crate) fn members(&mut self) -> impl Iterator<Item = &mut Payload> {
        let (job, chain) = match self {
            Self::Job(job) => (Some(job), None),
            Self::Chain(chain) => (None, Some(chain)),
        };
        job.into_iter().chain(chain.into_iter().flatten())
    }

    pub(crate) fn into_head(self, batch_id: &str) -> QueueResult<Payload> {
        let mut entry = self;
        for job in entry.members() {
            job.batch_id = Some(batch_id.to_owned());
        }
        match entry {
            Self::Job(job) => Ok(job),
            Self::Chain(mut chain) => {
                let head = chain
                    .pop_front()
                    .ok_or(crate::queue::QueueError::InvalidPayload)?;
                Ok(head.with_chain(chain, Vec::new()))
            }
        }
    }
}

/// Builds a batch for a single queue connection and queue.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingBatch {
    pub(crate) jobs: Vec<BatchJob>,
    pub(crate) name: String,
    pub(crate) options: Map<String, Value>,
}

#[allow(non_snake_case, reason = "Matches Laravel PendingBatch")]
impl PendingBatch {
    pub(super) fn new(jobs: impl IntoIterator<Item = Payload>) -> Self {
        Self {
            jobs: jobs.into_iter().map(BatchJob::Job).collect(),
            name: String::new(),
            options: Map::new(),
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    #[allow(
        clippy::should_implement_trait,
        reason = "Matches Laravel PendingBatch::add"
    )]
    pub fn add(mut self, jobs: impl IntoIterator<Item = Payload>) -> Self {
        self.jobs.extend(jobs.into_iter().map(BatchJob::Job));
        self
    }

    /// Appends a sequence whose members run one at a time in this batch.
    pub fn addChain(mut self, jobs: impl IntoIterator<Item = Payload>) -> Self {
        self.jobs.push(BatchJob::Chain(jobs.into_iter().collect()));
        self
    }

    pub fn allowFailures(mut self) -> Self {
        self.options.insert("allowFailures".into(), true.into());
        self
    }

    pub fn before(self, callback: impl Into<String>) -> Self {
        self.callback(BatchPhase::Before, callback.into())
    }

    pub fn progress(self, callback: impl Into<String>) -> Self {
        self.callback(BatchPhase::Progress, callback.into())
    }

    pub fn then(self, callback: impl Into<String>) -> Self {
        self.callback(BatchPhase::Then, callback.into())
    }

    pub fn catch(self, callback: impl Into<String>) -> Self {
        self.callback(BatchPhase::Catch, callback.into())
    }

    pub fn finally(self, callback: impl Into<String>) -> Self {
        self.callback(BatchPhase::Finally, callback.into())
    }

    /// Allows failures and registers a callback for each terminal job failure.
    pub fn onFailure(self, callback: impl Into<String>) -> Self {
        self.allowFailures()
            .callback(BatchPhase::Failure, callback.into())
    }

    fn callback(mut self, phase: BatchPhase, name: String) -> Self {
        let names = self
            .options
            .entry(phase.key())
            .or_insert_with(|| Value::Array(Vec::new()));
        names
            .as_array_mut()
            .expect("callback names")
            .push(name.into());
        self
    }

    pub fn onConnection(mut self, name: impl Into<String>) -> Self {
        self.options.insert("connection".into(), name.into().into());
        self
    }

    pub fn onQueue(mut self, name: impl Into<String>) -> Self {
        self.options.insert("queue".into(), name.into().into());
        self
    }

    pub async fn dispatch(self) -> QueueResult<Batch> {
        Queue::manager()?
            .dispatch_batch(self.name, self.options, self.jobs)
            .await
    }
}
