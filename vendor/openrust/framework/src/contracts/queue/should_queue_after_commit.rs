use super::ShouldQueue;

/// Marks a queued job or listener for enqueueing after a successful commit.
pub trait ShouldQueueAfterCommit: ShouldQueue {}
