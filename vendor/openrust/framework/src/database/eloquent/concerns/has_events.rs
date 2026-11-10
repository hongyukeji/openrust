use crate::database::{ConnectionInterface, DatabaseFuture, DatabaseResult};

use super::super::Model;

/// An Eloquent lifecycle event.
pub enum ModelEvent {
    Retrieved,
    Creating,
    Created,
    Updating,
    Updated,
    Saving,
    Saved,
    Restoring,
    Restored,
    Replicating,
    Trashed,
    Deleting,
    Deleted,
    ForceDeleting,
    ForceDeleted,
}

/// A listener's response to a model event.
pub enum ModelEventResponse {
    Continue,
    Stop,
    Cancel,
}

/// Handles model lifecycle events with access to the current operation's connection.
pub trait ModelEventListener<M: Model>: Send + Sync {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        connection: Option<&'a mut dyn ConnectionInterface>,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>>;
}
