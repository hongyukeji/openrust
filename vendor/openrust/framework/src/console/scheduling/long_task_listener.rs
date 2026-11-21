use super::Event;
use crate::{
    contracts::events::{Event as EventContract, Listener},
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
};
use std::{sync::Arc, time::Duration};

pub(super) type Handler = dyn Fn(&Arc<Event>, Duration) -> EventResult<()> + Send + Sync;
pub(super) struct LongTaskListener {
    pub threshold: Duration,
    pub handler: Arc<Handler>,
}
impl Listener for LongTaskListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventContract,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let completed = event
                .downcast_ref::<crate::console::events::ScheduledTaskFinished>()
                .map(|event| (&event.task, event.runtime));
            let completed = completed.or_else(|| {
                event
                    .downcast_ref::<crate::console::events::ScheduledTaskFailed>()
                    .and_then(|event| event.runtime.map(|runtime| (&event.task, runtime)))
            });
            if let Some((task, runtime)) = completed
                && runtime > self.threshold
            {
                (self.handler)(task, runtime)?;
            }
            Ok(None)
        })
    }
}
