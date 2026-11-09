use std::sync::{Arc, Mutex};

use super::{Dispatcher, EventServiceProvider, EventsServiceProvider};
use crate::Result;
use crate::config::QueueConfig;
use crate::database::DatabaseTransaction;
use crate::foundation::ProviderBuilder;
use crate::foundation::RegisterContext;
use crate::queue::DispatchOptions;
use crate::queue::Job;
use crate::queue::JobContext;
use crate::queue::Queue;
use crate::queue::QueueDriver;
use crate::support::{BoxFuture, ServiceProvider};
use serde_json::Value;

struct QueueBinding(Queue);
impl ServiceProvider for QueueBinding {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(self.0.clone())).await?;
            Ok(())
        })
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct UserRegistered {
    user_id: i64,
}

impl super::Event for UserRegistered {
    const NAME: &'static str = "Tests\\UserRegistered";
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

struct LateQueue(Queue);
impl crate::support::ServiceProvider for LateQueue {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(self.0.clone())).await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn provider_resolves_queue_registered_later_and_fails_when_missing() -> Result<()> {
    fn events_provider() -> super::EventsServiceProvider {
        let mut provider = super::EventsServiceProvider::new();
        provider.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
            Ok(WelcomeJob {
                account_id: event.user_id,
            })
        });
        provider
    }
    let mut missing = crate::foundation::ProviderBuilder::new();
    missing
        .provider(EventServiceProvider)
        .provider(events_provider());
    let error = missing
        .start(figment::Figment::new())
        .await
        .err()
        .ok_or_else(|| crate::Error::custom("test", "missing error"))?;
    assert_eq!(error.code(), "service_initialization_failed");
    match &error {
        crate::Error::ServiceInitialization { source, .. } => {
            assert_eq!(source.code(), "events_require_queue");
        }
        other => {
            return Err(crate::Error::custom(
                "test",
                format!("unexpected: {other:?}"),
            ));
        }
    }
    let driver = CaptureQueueDriver::default();
    let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder
        .provider(EventServiceProvider)
        .provider(events_provider())
        .provider(LateQueue(queue));
    let runtime = builder.start(figment::Figment::new()).await?;
    runtime
        .container()
        .resolve::<Dispatcher>()
        .await?
        .dispatch(&UserRegistered { user_id: 73 })
        .await?;
    assert_eq!(
        driver
            .dispatched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        1
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

struct BrokenQueue;
impl crate::support::ServiceProvider for BrokenQueue {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<Queue, _>(|resolver| {
                Box::pin(async move {
                    let _ = resolver.resolve::<u128>().await?;
                    Err(crate::Error::custom("test", "unreachable"))
                })
            })
            .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn superseded_startup_listeners_do_not_resolve_a_missing_queue() -> Result<()> {
    let mut superseded = EventsServiceProvider::new();
    superseded.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
        Ok(WelcomeJob {
            account_id: event.user_id,
        })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(EventServiceProvider)
        .provider(superseded)
        .force_provider(EventsServiceProvider::new());
    let runtime = builder.start(figment::Figment::new()).await?;
    let dispatcher = runtime.container().resolve::<Dispatcher>().await?;
    assert!(
        dispatcher
            .selected(<UserRegistered as crate::events::Event>::NAME, false)?
            .is_empty()
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn bound_queue_internal_missing_dependency_is_not_misreported() -> Result<()> {
    let mut events = super::EventsServiceProvider::new();
    events.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
        Ok(WelcomeJob {
            account_id: event.user_id,
        })
    });
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder
        .provider(EventServiceProvider)
        .provider(events)
        .provider(BrokenQueue);
    let error = builder
        .start(figment::Figment::new())
        .await
        .err()
        .ok_or_else(|| crate::Error::custom("test", "missing error"))?;
    assert_eq!(error.code(), "service_not_bound");
    assert!(!error.to_string().contains("queued listeners require"));
    Ok(())
}

#[tokio::test]
async fn queued_listener_maps_event_to_registered_job_payload() -> Result<()> {
    let driver = CaptureQueueDriver::default();
    let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
    let mut builder = ProviderBuilder::new();
    builder.provider(QueueBinding(queue));

    let mut plugin = EventsServiceProvider::new();
    plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
        Ok(WelcomeJob {
            account_id: event.user_id,
        })
    });
    builder.provider(EventServiceProvider).provider(plugin);
    let app = builder.start(figment::Figment::new()).await?;
    let events = app.container().resolve::<Dispatcher>().await?;
    events
        .dispatch_direct(&UserRegistered { user_id: 99 })
        .await?;
    events
        .dispatch_named(
            <UserRegistered as super::Event>::NAME,
            &UserRegistered { user_id: 73 },
        )
        .await?;

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
    let mut builder = ProviderBuilder::new();
    builder.provider(QueueBinding(queue));

    let later_listener_called = Arc::new(Mutex::new(false));
    let mut plugin = EventsServiceProvider::new();
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
    builder.provider(EventServiceProvider).provider(plugin);
    let app = builder.start(figment::Figment::new()).await?;
    let events = app.container().resolve::<Dispatcher>().await?;

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
    let mut builder = ProviderBuilder::new();
    builder.provider(QueueBinding(queue));

    let later_listener_called = Arc::new(Mutex::new(false));
    let mut plugin = EventsServiceProvider::new();
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
    builder.provider(EventServiceProvider).provider(plugin);
    let app = builder.start(figment::Figment::new()).await?;
    let events = app.container().resolve::<Dispatcher>().await?;

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
