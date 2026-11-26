use std::sync::Arc;

use crate::events::{
    Delivery, DispatchResult, EventContext, EventFuture, EventResult, ListenerResponse,
};

use super::{Event, Listener, Subscriber};

/// Registers listeners and delivers events.
pub trait Dispatcher: Send + Sync {
    fn listen(
        &self,
        event: &str,
        listener: Arc<dyn Listener>,
        delivery: Delivery,
    ) -> EventResult<()>;
    fn has_listeners(&self, event: &str) -> bool;
    fn subscribe(&self, subscriber: &dyn Subscriber) -> EventResult<()>;
    fn forget(&self, event: &str);

    fn dispatch<'a>(
        &'a self,
        event: Arc<dyn Event>,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<DispatchResult>>;

    fn until<'a>(
        &'a self,
        event: Arc<dyn Event>,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>;
}
