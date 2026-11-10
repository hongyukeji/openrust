use crate::events::{EventContext, EventFuture, EventResult, ListenerResponse};

use super::Event;

/// Handles an event in the current execution scope.
pub trait Listener: Send + Sync {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>;
}
