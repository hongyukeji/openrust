use crate::events::{EventContext, EventFuture, EventResult, ListenerResponse};

use super::Event;

/// Handles an event in the current execution scope.
pub trait Listener: Send + Sync {
    /// 返回监听器的具体类型，用于队列观测。
    fn queue_subject_type(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>;
}

impl<F> Listener for F
where
    F: for<'a, 'tx> Fn(
            &'a dyn Event,
            &'a mut EventContext<'tx>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>
        + Send
        + Sync,
{
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        self(event, context)
    }
}
