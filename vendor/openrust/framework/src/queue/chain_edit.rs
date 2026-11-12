use std::collections::{HashSet, VecDeque};

use crate::bus::{ChainedBatch, PendingBatch};

use super::{
    DispatchOptions, JobChain, JobContext, Payload, QueueError, QueueManager, QueueResult,
};

/// A job or pending batch inserted while a chain is running.
#[allow(
    clippy::large_enum_variant,
    reason = "Keeps the ordinary job payload inline"
)]
pub enum ChainJob {
    Job(Payload),
    Batch(PendingBatch),
}

impl From<Payload> for ChainJob {
    fn from(job: Payload) -> Self {
        Self::Job(job)
    }
}

impl From<PendingBatch> for ChainJob {
    fn from(batch: PendingBatch) -> Self {
        Self::Batch(batch)
    }
}

impl ChainJob {
    fn into_payload(self) -> QueueResult<Payload> {
        match self {
            Self::Job(job) if job.is_chain_member() => Ok(job),
            Self::Job(_) => Err(QueueError::InvalidPayload),
            Self::Batch(batch) => ChainedBatch::payload(batch),
        }
    }
}

#[derive(Default)]
pub(super) struct ChainEdits {
    prepend: VecDeque<Payload>,
    append: VecDeque<Payload>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ChainUpdate {
    pub(crate) added: u32,
    pub(crate) next: Payload,
}

#[allow(non_snake_case, reason = "Matches Laravel Queueable")]
impl JobContext<'_> {
    /// Runs a job or batch immediately after this job succeeds.
    pub fn prependToChain(&mut self, job: impl Into<ChainJob>) -> QueueResult<()> {
        self.chain_edits
            .prepend
            .push_front(job.into().into_payload()?);
        Ok(())
    }

    /// Runs a job or batch after the remaining chain succeeds.
    pub fn appendToChain(&mut self, job: impl Into<ChainJob>) -> QueueResult<()> {
        self.chain_edits
            .append
            .push_back(job.into().into_payload()?);
        Ok(())
    }

    pub(super) fn finish_chain_edit(&mut self, job: &Payload) -> QueueResult<()> {
        let ChainEdits {
            mut prepend,
            mut append,
        } = std::mem::take(&mut self.chain_edits);
        if prepend.is_empty() && append.is_empty() {
            return Ok(());
        }
        let mut inserted = prepend.iter_mut().chain(&mut append);
        if let Some(batch_id) = job.batchId() {
            for step in &mut inserted {
                if step.chained_batch.is_some() {
                    return Err(QueueError::InvalidPayload);
                }
                step.batch_id = Some(batch_id.to_owned());
            }
        } else if inserted.any(|step| step.chained_batch.is_some()) {
            let manager = crate::support::facades::Queue::manager()?;
            self.validate_batch_edit(&manager, &prepend, &append)?;
        }
        let added = u32::try_from(prepend.len().saturating_add(append.len()))
            .map_err(|_| QueueError::InvalidPayload)?;
        let JobChain {
            mut remaining,
            catches,
        } = job.chain.clone().unwrap_or_default();
        prepend.append(&mut remaining);
        prepend.append(&mut append);
        let mut ids = HashSet::with_capacity(prepend.len() + 1);
        ids.insert(job.uuid());
        if prepend.iter().any(|step| !ids.insert(step.uuid())) {
            return Err(QueueError::InvalidPayload);
        }
        let mut next = prepend.pop_front().ok_or(QueueError::InvalidPayload)?;
        next.context.clone_from(&job.context);
        next.chain_route.clone_from(&job.chain_route);
        self.chain_update = Some(ChainUpdate {
            added,
            next: next.with_chain(prepend, catches),
        });
        Ok(())
    }

    fn validate_batch_edit(
        &self,
        manager: &QueueManager,
        prepend: &VecDeque<Payload>,
        append: &VecDeque<Payload>,
    ) -> QueueResult<()> {
        let options = DispatchOptions {
            connection: Some(self.connection.clone()),
            queue: Some(self.queue.clone()),
            ..Default::default()
        };
        manager.validate_chained_batches(prepend.iter().chain(append), &options)
    }
}
