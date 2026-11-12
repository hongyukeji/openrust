use std::{marker::PhantomData, sync::Arc, time::Duration};

use crate::{
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
};

use super::{
    QueueFacade, QueueManager, QueueResult, backend,
    events::{
        JobAttempted, JobExceptionOccurred, JobFailed, JobProcessed, JobProcessing, JobSnapshot,
        Looping, WorkerStarting, WorkerStopping,
    },
};

struct WorkerListener<E, F> {
    callback: F,
    event: PhantomData<fn() -> E>,
}

impl<E: Event, F> Listener for WorkerListener<E, F>
where
    F: for<'a> Fn(&'a E) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> + Send + Sync,
{
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        match event.downcast_ref::<E>() {
            Some(event) => (self.callback)(event),
            None => Box::pin(async { Err(EventError::InvalidPayload) }),
        }
    }
}

impl QueueManager {
    fn listen_worker_event<E: Event>(
        &self,
        name: &str,
        callback: impl for<'a> Fn(&'a E) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>
        + Send
        + Sync
        + 'static,
    ) -> QueueResult<()> {
        let (_, dispatcher) = self.worker_dispatcher()?;
        dispatcher
            .listen(
                name,
                Arc::new(WorkerListener {
                    callback,
                    event: PhantomData,
                }),
                Delivery::Sync,
            )
            .map_err(backend)
    }

    /// Calls the listener after a measured attempt takes strictly longer than the threshold.
    #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
    pub fn whenJobRunsLongerThan(
        &self,
        threshold: Duration,
        callback: impl for<'a> Fn(
            &'a JobSnapshot,
            Duration,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>
        + Send
        + Sync
        + 'static,
    ) -> QueueResult<()> {
        self.listen_worker_event(JobAttempted::NAME, move |event: &JobAttempted| {
            let duration = event
                .duration
                .and_then(|ms| Duration::try_from_secs_f64(ms / 1000.0).ok());
            match duration {
                Some(duration) if duration > threshold => callback(&event.job, duration),
                _ => Box::pin(async { Ok(None) }),
            }
        })
    }
}

impl QueueFacade {
    /// Calls the listener after a measured attempt takes strictly longer than the threshold.
    #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
    pub fn whenJobRunsLongerThan(
        threshold: Duration,
        callback: impl for<'a> Fn(
            &'a JobSnapshot,
            Duration,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>
        + Send
        + Sync
        + 'static,
    ) -> QueueResult<()> {
        Self::manager()?.whenJobRunsLongerThan(threshold, callback)
    }
}

macro_rules! lifecycle_listeners {
    ($($method:ident => $event:ident),+ $(,)?) => {
        impl QueueManager {
            $(
                #[doc = concat!("Registers a typed listener for `", stringify!($event), "`.")]
                #[allow(non_snake_case, reason = "Matches Laravel QueueManager")]
                pub fn $method(
                    &self,
                    callback: impl for<'a> Fn(&'a $event) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>
                    + Send + Sync + 'static,
                ) -> QueueResult<()> {
                    self.listen_worker_event($event::NAME, callback)
                }
            )+
        }

        impl QueueFacade {
            $(
                #[doc = concat!("Registers a typed listener for `", stringify!($event), "`.")]
                #[allow(non_snake_case, reason = "Matches Laravel Queue facade")]
                pub fn $method(
                    callback: impl for<'a> Fn(&'a $event) -> EventFuture<'a, EventResult<Option<ListenerResponse>>>
                    + Send + Sync + 'static,
                ) -> QueueResult<()> {
                    Self::manager()?.$method(callback)
                }
            )+
        }
    };
}

lifecycle_listeners!(
    before => JobProcessing,
    after => JobProcessed,
    exceptionOccurred => JobExceptionOccurred,
    looping => Looping,
    failing => JobFailed,
    starting => WorkerStarting,
    stopping => WorkerStopping,
);
