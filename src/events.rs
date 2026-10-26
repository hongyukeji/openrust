//! Typed, ordered in-process events with explicit durable queue listeners.
//!
//! In-process listeners run in registration order and stop at the first error. They are not
//! persisted or broadcast across processes. Use a queued listener inside the business transaction
//! when delivery must survive a process exit.

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    sync::Arc,
};

use crate::{
    core::{App, AppBuilder, BoxFuture, Error, Plugin, Result},
    db::DatabaseTransaction,
};

/// Context available to an event listener.
#[derive(Clone, Copy)]
pub struct EventContext<'a> {
    transaction: Option<&'a DatabaseTransaction>,
}

impl<'a> EventContext<'a> {
    /// Returns the caller's transaction when dispatch used [`Events::dispatch_in_transaction`].
    pub const fn transaction(&self) -> Option<&'a DatabaseTransaction> {
        self.transaction
    }
}

trait ErasedListener: Send + Sync {
    fn event_type(&self) -> TypeId;

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
    ) -> BoxFuture<'a, Result<()>>;
}

struct TypedListener<E, F> {
    handler: F,
    marker: PhantomData<fn(&E)>,
}

impl<E, F> ErasedListener for TypedListener<E, F>
where
    E: Any + Send + Sync + 'static,
    F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<()>> + Send + Sync,
{
    fn event_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
    ) -> BoxFuture<'a, Result<()>> {
        let Some(event) = event.downcast_ref::<E>() else {
            return Box::pin(async {
                Err(Error::custom(
                    "event_type_mismatch",
                    "event listener received an incompatible event type",
                ))
            });
        };
        (self.handler)(event, EventContext { transaction })
    }
}

enum ListenerRegistration {
    Direct(Arc<dyn ErasedListener>),
    #[cfg(feature = "queue")]
    Queued {
        build: Box<dyn Fn(crate::queue::Queue) -> Arc<dyn ErasedListener> + Send + Sync>,
    },
}

/// Registers application event listeners in deterministic order.
#[derive(Default)]
pub struct EventsPlugin {
    listeners: Vec<ListenerRegistration>,
}

impl EventsPlugin {
    /// Creates an empty listener registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an async listener for one concrete event type.
    pub fn listen<E, F>(&mut self, handler: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<()>> + Send + Sync + 'static,
    {
        self.listeners
            .push(ListenerRegistration::Direct(Arc::new(TypedListener::<
                E,
                F,
            > {
                handler,
                marker: PhantomData,
            })));
        self
    }

    /// Registers a durable listener that maps an event to a registered queue job.
    ///
    /// `QueuePlugin` must be registered before this plugin, and the returned job is executed
    /// at least once by an explicitly started worker. Inside a transaction dispatch, the job
    /// insert uses the same transaction and is removed if that transaction rolls back.
    #[cfg(feature = "queue")]
    pub fn listen_queued<E, J, F>(&mut self, queue_name: impl Into<String>, map: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        J: crate::queue::Job + serde::Serialize,
        F: Fn(&E) -> Result<J> + Send + Sync + 'static,
    {
        let queue_name = queue_name.into();
        let map = Arc::new(map);
        self.listeners.push(ListenerRegistration::Queued {
            build: Box::new(move |queue| {
                Arc::new(QueuedListener::<E, J, F> {
                    queue,
                    queue_name: queue_name.clone(),
                    map: Arc::clone(&map),
                    marker: PhantomData,
                })
            }),
        });
        self
    }
}

#[cfg(feature = "queue")]
struct QueuedListener<E, J, F> {
    queue: crate::queue::Queue,
    queue_name: String,
    map: Arc<F>,
    marker: PhantomData<fn(&E) -> J>,
}

#[cfg(feature = "queue")]
impl<E, J, F> ErasedListener for QueuedListener<E, J, F>
where
    E: Any + Send + Sync + 'static,
    J: crate::queue::Job + serde::Serialize,
    F: Fn(&E) -> Result<J> + Send + Sync,
{
    fn event_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
    ) -> BoxFuture<'a, Result<()>> {
        let Some(event) = event.downcast_ref::<E>() else {
            return Box::pin(async {
                Err(Error::custom(
                    "event_type_mismatch",
                    "event listener received an incompatible event type",
                ))
            });
        };
        let job = match (self.map)(event) {
            Ok(job) => job,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        Box::pin(async move {
            if let Some(transaction) = transaction {
                self.queue
                    .dispatch_in_transaction(transaction, &self.queue_name, J::NAME, &job)
                    .await?;
            } else {
                self.queue.dispatch(&self.queue_name, J::NAME, &job).await?;
            }
            Ok(())
        })
    }
}

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<Events>().is_some() {
            return Err(Error::custom(
                "duplicate_events_plugin",
                "register only one EventsPlugin per application",
            ));
        }
        #[cfg(feature = "queue")]
        let mut queue = None;
        let mut listeners = Vec::with_capacity(self.listeners.len());
        for registration in &self.listeners {
            match registration {
                ListenerRegistration::Direct(listener) => listeners.push(Arc::clone(listener)),
                #[cfg(feature = "queue")]
                ListenerRegistration::Queued { build } => {
                    if queue.is_none() {
                        queue = app.get::<crate::queue::Queue>();
                    }
                    let queue = queue.as_ref().ok_or_else(|| {
                        Error::custom(
                            "events_require_queue",
                            "register QueuePlugin before EventsPlugin with queued listeners",
                        )
                    })?;
                    listeners.push(build((**queue).clone()));
                }
            }
        }
        app.insert(Events { listeners });
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.state().try_get::<Events>()?;
            Ok(())
        })
    }
}

/// Application-scoped typed event dispatcher.
#[derive(Clone)]
pub struct Events {
    listeners: Vec<Arc<dyn ErasedListener>>,
}

impl Events {
    /// Delivers an event to matching listeners in registration order.
    ///
    /// The first listener error stops dispatch and propagates to the caller. This process-local
    /// operation is neither persisted nor broadcast across application processes.
    pub async fn dispatch<E>(&self, event: &E) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        self.dispatch_with_transaction(event, None).await
    }

    /// Delivers an event with the caller's active transaction in its listener context.
    ///
    /// Propagate errors from this method out of the transaction callback to roll back business
    /// writes and any queued listener jobs. This method does not commit the transaction. A direct
    /// listener must not perform irreversible external side effects here; use `listen_queued`.
    pub async fn dispatch_in_transaction<E>(
        &self,
        transaction: &DatabaseTransaction,
        event: &E,
    ) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        self.dispatch_with_transaction(event, Some(transaction))
            .await
    }

    async fn dispatch_with_transaction<E>(
        &self,
        event: &E,
        transaction: Option<&DatabaseTransaction>,
    ) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        for listener in &self.listeners {
            if listener.event_type() == TypeId::of::<E>() {
                listener.call(event, transaction).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{Events, EventsPlugin};
    use crate::{AppBuilder, Plugin, config::FrameworkConfig};
    use figment::Figment;

    #[derive(Debug)]
    struct Probe(u8);

    fn events(plugin: &EventsPlugin) -> crate::Result<Events> {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        plugin.build(&mut builder)?;
        let app = builder.build();
        Ok((*app.state().try_get::<Events>()?).clone())
    }

    #[tokio::test]
    async fn listeners_run_in_order_and_stop_after_the_first_error() -> crate::Result<()> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut plugin = EventsPlugin::new();
        let first = Arc::clone(&calls);
        plugin.listen::<Probe, _>(move |event, _| {
            let calls = Arc::clone(&first);
            Box::pin(async move {
                calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.0);
                Err(crate::Error::custom(
                    "rejected",
                    "probe listener rejected the event",
                ))
            })
        });
        let second = Arc::clone(&calls);
        plugin.listen::<Probe, _>(move |event, _| {
            let calls = Arc::clone(&second);
            Box::pin(async move {
                calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.0 + 1);
                Ok(())
            })
        });
        let dispatcher = events(&plugin)?;
        assert!(dispatcher.dispatch(&Probe(4)).await.is_err());
        assert_eq!(
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_slice(),
            [4]
        );
        Ok(())
    }

    #[tokio::test]
    async fn unrelated_event_types_are_not_delivered() -> crate::Result<()> {
        #[derive(Debug)]
        struct Other;
        let calls = Arc::new(Mutex::new(0));
        let observed = Arc::clone(&calls);
        let mut plugin = EventsPlugin::new();
        plugin.listen::<Probe, _>(move |_, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                *observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
                Ok(())
            })
        });
        events(&plugin)?.dispatch(&Other).await?;
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            0
        );
        Ok(())
    }
}

#[cfg(all(test, feature = "queue"))]
mod queued_tests {
    use std::sync::{Arc, Mutex};

    use crate::{
        AppBuilder, BoxFuture, Plugin, Result,
        config::{FrameworkConfig, QueueConfig},
        db::DatabaseTransaction,
        events::{Events, EventsPlugin},
        queue::{DispatchOptions, Job, JobContext, Queue, QueueDriver},
    };
    use serde_json::Value;

    #[derive(serde::Deserialize, serde::Serialize)]
    struct UserRegistered {
        user_id: i64,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct WelcomeJob {
        account_id: i64,
    }

    impl Job for WelcomeJob {
        const NAME: &'static str = "tests.events.welcome.v1";

        fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Clone, Default)]
    struct CaptureQueueDriver {
        dispatched: Arc<Mutex<Vec<(String, String, Value)>>>,
        fail_dispatch: bool,
    }

    impl QueueDriver for CaptureQueueDriver {
        fn dispatch<'a>(
            &'a self,
            queue: &'a str,
            job_name: &'a str,
            payload: Value,
            _options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                if self.fail_dispatch {
                    return Err(crate::Error::custom(
                        "queue_unavailable",
                        "queue store rejected dispatch",
                    ));
                }
                self.dispatched
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((queue.to_owned(), job_name.to_owned(), payload));
                Ok("captured-job".to_owned())
            })
        }

        fn dispatch_in_transaction<'a>(
            &'a self,
            _transaction: &'a DatabaseTransaction,
            queue: &'a str,
            job_name: &'a str,
            payload: Value,
            options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            self.dispatch(queue, job_name, payload, options)
        }

        fn run_next<'a>(&'a self, _queue: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }

        fn retry_failed<'a>(&'a self, _failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Ok("captured-retry".to_owned()) })
        }

        fn reset_shutdown(&self) {}

        fn request_shutdown(&self) {}

        fn is_shutdown_requested(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn queued_listener_maps_event_to_registered_job_payload() -> Result<()> {
        let driver = CaptureQueueDriver::default();
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
        builder.insert(queue);

        let mut plugin = EventsPlugin::new();
        plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
            Ok(WelcomeJob {
                account_id: event.user_id,
            })
        });
        plugin.build(&mut builder)?;
        let app = builder.build();
        let events = app.state().try_get::<Events>()?;
        events.dispatch(&UserRegistered { user_id: 73 }).await?;

        let dispatched = driver
            .dispatched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].0, "mail");
        assert_eq!(dispatched[0].1, WelcomeJob::NAME);
        assert_eq!(dispatched[0].2, serde_json::json!({"account_id": 73}));
        Ok(())
    }

    #[tokio::test]
    async fn queued_listener_mapping_error_stops_later_listeners() -> Result<()> {
        let driver = CaptureQueueDriver::default();
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
        builder.insert(queue);

        let later_listener_called = Arc::new(Mutex::new(false));
        let mut plugin = EventsPlugin::new();
        plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |_| {
            Err(crate::Error::custom(
                "mapping_rejected",
                "event cannot be mapped to a welcome job",
            ))
        });
        let called = Arc::clone(&later_listener_called);
        plugin.listen::<UserRegistered, _>(move |_, _| {
            let called = Arc::clone(&called);
            Box::pin(async move {
                *called
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Ok(())
            })
        });
        plugin.build(&mut builder)?;
        let app = builder.build();
        let events = app.state().try_get::<Events>()?;

        assert!(
            events
                .dispatch(&UserRegistered { user_id: 73 })
                .await
                .is_err()
        );
        assert!(
            driver
                .dispatched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert!(
            !*later_listener_called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        );
        Ok(())
    }

    #[tokio::test]
    async fn queued_dispatch_error_stops_later_listeners() -> Result<()> {
        let driver = CaptureQueueDriver {
            fail_dispatch: true,
            ..CaptureQueueDriver::default()
        };
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
        builder.insert(queue);

        let later_listener_called = Arc::new(Mutex::new(false));
        let mut plugin = EventsPlugin::new();
        plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
            Ok(WelcomeJob {
                account_id: event.user_id,
            })
        });
        let called = Arc::clone(&later_listener_called);
        plugin.listen::<UserRegistered, _>(move |_, _| {
            let called = Arc::clone(&called);
            Box::pin(async move {
                *called
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Ok(())
            })
        });
        plugin.build(&mut builder)?;
        let app = builder.build();
        let events = app.state().try_get::<Events>()?;

        assert!(
            events
                .dispatch(&UserRegistered { user_id: 73 })
                .await
                .is_err()
        );
        assert!(
            driver
                .dispatched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert!(
            !*later_listener_called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        );
        Ok(())
    }
}
