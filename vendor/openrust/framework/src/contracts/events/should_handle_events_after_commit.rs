use super::Listener;

/// Marks a listener for execution after a successful commit.
pub trait ShouldHandleEventsAfterCommit: Listener {}
