use crate::queue::{QueueFuture, QueueResult};

use super::Batch;

/// A named callback compiled into both the application and the queue worker.
/// The optional string contains the terminal job failure for failure callbacks.
pub type BatchCallback = for<'a> fn(&'a Batch, Option<&'a str>) -> QueueFuture<'a, QueueResult<()>>;

#[derive(Clone, Copy)]
pub(crate) enum BatchPhase {
    Before,
    Progress,
    Then,
    Catch,
    Finally,
    Failure,
}

impl BatchPhase {
    pub(crate) const ALL: [Self; 6] = [
        Self::Before,
        Self::Progress,
        Self::Then,
        Self::Catch,
        Self::Finally,
        Self::Failure,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Progress => "progress",
            Self::Then => "then",
            Self::Catch => "catch",
            Self::Finally => "finally",
            Self::Failure => "failure",
        }
    }

    pub(crate) fn takes_failure(self) -> bool {
        matches!(self, Self::Progress | Self::Catch | Self::Failure)
    }

    pub(crate) fn names(self, batch: &Batch) -> impl Iterator<Item = &str> {
        batch
            .options
            .get(self.key())
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
    }
}

pub(crate) struct BatchTransition {
    pub(crate) batch: Batch,
    pub(crate) phases: Vec<BatchPhase>,
}
