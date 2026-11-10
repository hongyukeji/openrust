use super::Event;

/// Marks an event for dispatch after a successful commit.
pub trait ShouldDispatchAfterCommit: Event {}
