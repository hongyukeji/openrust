use serde_json::{Map, Value};

use crate::queue::{Payload, QueueResult};

use super::PendingBatch;

/// Batch definition carried by a queued step until the worker dispatches it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ChainedBatch {
    pub(crate) pending: PendingBatch,
}

impl ChainedBatch {
    pub(crate) fn payload(pending: PendingBatch) -> QueueResult<Payload> {
        let mut payload = Payload::new("bus.chained-batch", ())?;
        payload.chained_batch = Some(Box::new(Self { pending }));
        Ok(payload)
    }

    pub(crate) fn options(&self, connection: &str, queue: &str) -> Map<String, Value> {
        let mut options = self.pending.options.clone();
        options
            .entry("connection")
            .or_insert_with(|| connection.into());
        options.entry("queue").or_insert_with(|| queue.into());
        options
    }
}

/// Data kept separately from the public batch options until the batch settles.
pub(crate) struct BatchContinuation {
    pub(crate) id: String,
    pub(crate) connection: String,
    pub(crate) queue: String,
    pub(crate) next: Option<Payload>,
    pub(crate) catches: Vec<String>,
}
