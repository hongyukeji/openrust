use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::{Ttl, rate_limiting::Limit},
    contracts::{
        container::Container as _,
        database::LostConnectionDetector,
        events::{Event as PayloadEvent, Listener},
        foundation::Application as _,
        http::Kernel as _,
        queue::{Interruptible, JobHandler},
    },
    database::{DB, DatabaseManager, Value},
    events::{
        Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerRegistry,
        ListenerResponse, QueuedListenerOptions,
    },
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        events::DiscoverEvents,
        http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{
        DispatchOptions, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueManager,
        QueueResult, WorkerStopReason,
        events::{
            JobAttempted, JobExceptionOccurred, JobFailed, JobInterrupted, JobPopped, JobPopping,
            JobProcessed, JobProcessing, JobReleased, JobReleasedAfterException, JobTimedOut,
            Looping, QueuePaused, QueueResumed, QueuesPaused, QueuesResumed, WorkerIdle,
            WorkerInterrupted, WorkerPausing, WorkerQueuePaused, WorkerQueueResumed,
            WorkerResuming, WorkerStarting, WorkerStopping,
        },
        middleware::{
            ExceptionThrottleLimiter, JobMiddleware, JobNext, RateLimited, RateLimitedWithRedis,
            Release, Skip, ThrottlesExceptions, ThrottlesExceptionsWithRedis, WithoutOverlapping,
        },
    },
    routing::{Router, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Context, Event, Queue, RateLimiter},
    },
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
    net::{TcpListener, TcpStream},
    process::Command,
    time::{sleep, timeout},
};

fn execution(source: impl std::error::Error + Send + Sync + 'static) -> QueueError {
    QueueError::Execution {
        source: Box::new(source),
    }
}

struct Tag;
impl JobMiddleware for Tag {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            assert!(Context::get("middleware").map_err(execution)?.is_none());
            Context::add("middleware", json!("tagged")).map_err(execution)?;
            next.run(context, payload).await
        })
    }
}

struct Handler;
impl JobHandler for Handler {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        vec![Arc::new(Tag)]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            assert_eq!(
                Context::get("trace").map_err(execution)?,
                Some(json!(payload.name()))
            );
            assert_eq!(
                Context::get("hydrated").map_err(execution)?,
                Some(json!(true))
            );
            assert_eq!(
                Context::get("middleware").map_err(execution)?,
                Some(json!("tagged"))
            );
            assert_eq!(
                Context::getHidden("protected").map_err(execution)?,
                Some(json!("hidden"))
            );
            assert!(Context::getHidden("secret").map_err(execution)?.is_none());
            assert!(
                Context::get("previous_attempt")
                    .map_err(execution)?
                    .is_none()
            );
            Context::add("previous_attempt", json!(context.attempts())).map_err(execution)?;
            let db = DB::connection(None).map_err(execution)?;
            db.statement(
                "insert into receipts (name, value) values (?, ?)",
                &[payload.name().into(), i64::from(context.attempts()).into()],
            )
            .await
            .map_err(execution)?;
            match payload.name() {
                "timeout" | "retry_timeout" if context.attempts() == 1 => {
                    sleep(Duration::from_secs(10)).await;
                    db.statement("insert into receipts (name, value) values ('late', 1)", &[])
                        .await
                        .map_err(execution)?;
                }
                "mysql_disconnect" if context.attempts() == 1 => {
                    let mut transaction = db.beginTransaction().await.map_err(execution)?;
                    let id = transaction
                        .scalar("SELECT CONNECTION_ID()", &[])
                        .await
                        .map_err(execution)?;
                    let id = match id {
                        Value::Integer(id) => id,
                        Value::Unsigned(id) => i64::try_from(id).unwrap(),
                        _ => panic!("unexpected connection ID"),
                    };
                    println!("lost-mysql:{id}");
                    transaction
                        .scalar("SELECT SLEEP(30)", &[])
                        .await
                        .map_err(execution)?;
                    transaction.commit().await.map_err(execution)?;
                }
                "manual_release" if context.attempts() == 1 => {
                    context.release(Duration::ZERO);
                }
                "flaky" if context.attempts() == 1 => {
                    return Err(execution(std::io::Error::other("retryable")));
                }
                "terminal"
                    if db
                        .table("receipts")
                        .r#where("name", "=", "allow")
                        .count("*")
                        .await
                        .map_err(execution)?
                        == 0 =>
                {
                    return Err(execution(std::io::Error::other("terminal")));
                }
                "pause_gate" | "restart_gate" | "signal_gate" => {
                    let release = format!("{}_release", payload.name());
                    while db
                        .table("receipts")
                        .r#where("name", "=", release.as_str())
                        .count("*")
                        .await
                        .map_err(execution)?
                        == 0
                    {
                        sleep(Duration::from_millis(20)).await;
                    }
                    db.statement(
                        "insert into receipts (name, value) values (?, 1)",
                        &[format!("{}_done", payload.name()).into()],
                    )
                    .await
                    .map_err(execution)?;
                }
                "slow" => {
                    sleep(Duration::from_millis(600)).await;
                    db.statement(
                        "insert into receipts (name, value) values ('slow_done', 1)",
                        &[],
                    )
                    .await
                    .map_err(execution)?;
                }
                _ => {}
            }
            Ok(())
        })
    }
}

#[derive(Default)]
struct InterruptibleHandler(tokio::sync::Notify);

impl JobHandler for InterruptibleHandler {
    fn interruptible(&self) -> Option<&dyn Interruptible> {
        Some(self)
    }

    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        Handler.middleware()
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            Handler.handle(context, payload).await?;
            self.0.notified().await;
            Ok(())
        })
    }
}

impl Interruptible for InterruptibleHandler {
    fn interrupted(&self, signal: i32) -> QueueResult<()> {
        let trace = Context::get("trace").map_err(execution)?.unwrap();
        let kind = trace.as_str().unwrap();
        assert!(matches!(
            kind,
            "interrupt_gate" | "interrupt_timeout" | "interrupt_error"
        ));
        println!("job-interruption:callback:{kind}:{signal}");
        if kind == "interrupt_error" {
            return Err(execution(std::io::Error::other("interrupt-hook-error")));
        }
        self.0.notify_one();
        Ok(())
    }
}

struct JobInterruptListener;

impl Listener for JobInterruptListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<JobInterrupted>()
                .ok_or(EventError::InvalidPayload)?;
            assert_eq!(event.job.queue, "default");
            assert_eq!(event.job.attempts, 1);
            assert!(matches!(
                event.connection_name.as_str(),
                "database" | "redis"
            ));
            assert!(!event.job.id.is_empty());
            println!(
                "job-interruption:event:{}:{}",
                event.job.payload.name(),
                event.signal
            );
            Ok(None)
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Invoice(u32);
impl PayloadEvent for Invoice {
    fn name(&self) -> &str {
        "invoice.paid"
    }
}

struct InvoiceListener;
impl Listener for InvoiceListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let invoice = event
                .downcast_ref::<Invoice>()
                .ok_or(EventError::InvalidPayload)?;
            assert_eq!(Context::get("trace").unwrap(), Some(json!("invoice")));
            assert_eq!(Context::get("hydrated").unwrap(), Some(json!(true)));
            let db = DB::connection(None).map_err(EventError::Transaction)?;
            db.statement(
                "insert into receipts (name, value) values ('invoice', ?)",
                &[i64::from(invoice.0).into()],
            )
            .await
            .map_err(EventError::Transaction)?;
            Ok(None)
        })
    }
}

struct ControlListener;
impl Listener for ControlListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let value = match event
                .downcast_ref::<QueuePaused>()
                .and_then(|event| event.ttl)
            {
                Some(Ttl::Seconds(seconds)) => seconds,
                _ => 1,
            };
            let (name, value) = if let Some(event) = event.downcast_ref::<WorkerStopping>() {
                if event.reason == WorkerStopReason::MaxMemoryExceeded {
                    assert_eq!(event.status, 12);
                    assert!(event.memory_usage.is_some_and(|memory| memory >= 1.0));
                } else if event.reason == WorkerStopReason::TimedOut {
                    assert_eq!(
                        event.status,
                        i32::from(event.worker_options.timed_out_exit_code)
                    );
                } else {
                    assert_eq!(event.status, 0);
                }
                assert_eq!(
                    event.last_job_processed_at.is_some(),
                    event.jobs_processed > 0
                );
                (
                    format!("stop:{:?}", event.reason),
                    i64::try_from(event.jobs_processed).unwrap(),
                )
            } else if let Some(event) = event.downcast_ref::<WorkerInterrupted>() {
                assert_eq!(event.queue, "default");
                assert!(matches!(
                    event.connection_name.as_str(),
                    "database" | "redis"
                ));
                (event.name().to_owned(), i64::from(event.signal))
            } else {
                (event.name().to_owned(), value)
            };
            let db = DB::connection(None).map_err(EventError::Transaction)?;
            db.statement(
                "insert into receipts (name, value) values (?, ?)",
                &[name.into(), value.into()],
            )
            .await
            .map_err(EventError::Transaction)?;
            Ok(None)
        })
    }
}

struct LoopListener {
    attempts: AtomicUsize,
    later: bool,
}

impl Listener for LoopListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<Looping>()
                .ok_or(EventError::InvalidPayload)?;
            assert_eq!(event.queue, "default");
            let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
            let mode = std::env::var("QUEUE_WORKER_VETO").unwrap_or_default();
            let veto = !self.later && (mode == "always" || (mode == "once" && attempt == 0));
            let name = if self.later {
                "loop:later"
            } else if veto {
                "loop:veto"
            } else {
                "loop:allowed"
            };
            DB::connection(None)
                .map_err(EventError::Transaction)?
                .statement(
                    "insert into receipts (name, value) values (?, 1)",
                    &[name.into()],
                )
                .await
                .map_err(EventError::Transaction)?;
            Ok(veto.then(|| ListenerResponse::Value(Box::new(false))))
        })
    }
}

struct PopListener;
impl Listener for PopListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let mode = std::env::var("QUEUE_WORKER_POP_EVENT").unwrap();
            let (phase, fail) = if let Some(event) = event.downcast_ref::<JobPopping>() {
                assert_eq!(event.queue.as_deref(), Some("default"));
                assert!(matches!(
                    event.connection_name.as_str(),
                    "database" | "redis"
                ));
                println!("pop-event:before");
                ("before", mode == "before-error")
            } else {
                let event = event
                    .downcast_ref::<JobPopped>()
                    .ok_or(EventError::InvalidPayload)?;
                assert_eq!(event.job.queue, "default");
                assert!(!event.job.id.is_empty());
                assert!(matches!(
                    event.connection_name.as_str(),
                    "database" | "redis"
                ));
                assert_eq!(event.job.payload.name(), "control_default");
                assert_eq!(event.job.attempts, if mode == "retry" { 2 } else { 1 });
                println!("pop-event:after:{}", event.job.attempts);
                ("after", mode == "after-error")
            };
            if fail {
                return Err(EventError::Listener {
                    source: Box::new(std::io::Error::other(format!("pop-event:{phase}-error"))),
                });
            }
            Ok(None)
        })
    }
}

const ATTEMPT_EVENTS: &[&str] = &[
    JobProcessing::NAME,
    JobProcessed::NAME,
    JobReleased::NAME,
    JobExceptionOccurred::NAME,
    JobReleasedAfterException::NAME,
    JobFailed::NAME,
    JobTimedOut::NAME,
    JobAttempted::NAME,
];

struct AttemptListener;
impl Listener for AttemptListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn PayloadEvent,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            macro_rules! record {
                ($($ty:ty),+ $(,)?) => {
                    $(if let Some(value) = event.downcast_ref::<$ty>() {
                        assert_eq!(value.job.queue, "default");
                        assert!(!value.job.id.is_empty());
                        assert!(matches!(value.connection_name.as_str(), "database" | "redis"));
                        println!("attempt-event:{}:{}:{}", <$ty>::NAME, value.job.payload.name(), value.job.attempts);
                    })+
                };
            }
            record!(
                JobProcessing,
                JobProcessed,
                JobReleased,
                JobExceptionOccurred,
                JobReleasedAfterException,
                JobFailed,
                JobTimedOut,
                JobAttempted,
            );
            if let Some(event) = event.downcast_ref::<JobAttempted>() {
                assert_eq!(event.successful(), event.exception.is_none());
                if event
                    .exception
                    .as_deref()
                    .is_some_and(|e| e.contains("attempt-event:preflight-error"))
                {
                    assert_eq!(event.duration, None);
                } else {
                    assert!(event.duration.is_some_and(|duration| duration >= 0.0));
                }
            }
            if let Some(event) = event.downcast_ref::<JobReleasedAfterException>() {
                assert_eq!(event.backoff, Duration::ZERO);
            }
            if let Some(event) = event.downcast_ref::<JobTimedOut>() {
                assert_eq!(event.timeout, Duration::from_millis(100));
            }
            let mode = std::env::var("QUEUE_WORKER_ATTEMPT_EVENTS").unwrap_or_default();
            let error = match (event.name(), mode.as_str()) {
                (JobProcessing::NAME, "preflight-error") => Some("attempt-event:preflight-error"),
                (JobProcessed::NAME, "processed-error") => Some("attempt-event:processed-error"),
                _ => None,
            };
            if let Some(error) = error {
                return Err(EventError::Listener {
                    source: Box::new(std::io::Error::other(error)),
                });
            }
            Ok(None)
        })
    }
}

fn listeners(registry: &mut ListenerRegistry) -> EventResult<()> {
    for event in [
        QueuePaused::NAME,
        QueueResumed::NAME,
        QueuesPaused::NAME,
        QueuesResumed::NAME,
        WorkerQueuePaused::NAME,
        WorkerQueueResumed::NAME,
        WorkerStarting::NAME,
        WorkerIdle::NAME,
        WorkerInterrupted::NAME,
        WorkerPausing::NAME,
        WorkerResuming::NAME,
        WorkerStopping::NAME,
    ] {
        registry.listen(event, Arc::new(ControlListener), Delivery::Sync)?;
    }
    if std::env::var_os("QUEUE_WORKER_VETO").is_some() {
        for later in [false, true] {
            registry.listen(
                Looping::NAME,
                Arc::new(LoopListener {
                    attempts: AtomicUsize::new(0),
                    later,
                }),
                Delivery::Sync,
            )?;
        }
    }
    if std::env::var_os("QUEUE_WORKER_POP_EVENT").is_some() {
        for name in [JobPopping::NAME, JobPopped::NAME] {
            registry.listen(name, Arc::new(PopListener), Delivery::Sync)?;
        }
    }
    if std::env::var_os("QUEUE_WORKER_ATTEMPT_EVENTS").is_some() {
        for name in ATTEMPT_EVENTS {
            registry.listen(name, Arc::new(AttemptListener), Delivery::Sync)?;
        }
    }
    registry.listen(
        JobInterrupted::NAME,
        Arc::new(JobInterruptListener),
        Delivery::Sync,
    )?;
    registry.listenQueued::<Invoice>(
        "invoice.paid",
        Arc::new(InvoiceListener),
        QueuedListenerOptions {
            listener_id: "invoice.audit".into(),
            connection: None,
            queue: None,
            delay: None,
            after_commit: Some(true),
        },
    )
}

struct FixtureLostConnectionDetector;

impl LostConnectionDetector for FixtureLostConnectionDetector {
    fn causedByLostConnection(&self, error: &(dyn std::error::Error + 'static)) -> bool {
        let mut current = Some(error);
        while let Some(error) = current {
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.to_string() == "retryable")
            {
                return true;
            }
            current = error.source();
        }
        false
    }
}

struct LimitedHandler(&'static str);

impl JobHandler for LimitedHandler {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        let limiter: Arc<dyn JobMiddleware> = match self.0 {
            "rate_drop" => Arc::new(RateLimited::new("job-drop").dontRelease()),
            "rate_unlimited" => Arc::new(RateLimited::new("job-unlimited")),
            "rate_missing" => Arc::new(RateLimited::new("job-missing")),
            "rate_redis" => Arc::new(
                RateLimitedWithRedis::new("job-redis")
                    .connection("default")
                    .dontRelease(),
            ),
            _ => Arc::new(RateLimited::new("job-budget").releaseAfter(Duration::from_secs(1))),
        };
        vec![limiter, Arc::new(Tag)]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Handler.handle(context, payload)
    }
}

struct ThrottledHandler(&'static str);

impl JobHandler for ThrottledHandler {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        let matches_error = |error: &QueueError, suffix| {
            std::error::Error::source(error)
                .is_some_and(|source| source.to_string().ends_with(suffix))
        };
        let throttle = ThrottlesExceptions::new(2, 8)
            .by("remote-service")
            .backoffWhen(|_| 1)
            .when(move |error, _| !matches_error(error, "_bypass"))
            .reportWhen(|_, limiter| {
                match limiter {
                    ExceptionThrottleLimiter::Cache(_) => println!("throttle-cache"),
                    ExceptionThrottleLimiter::Redis {
                        remaining,
                        decays_at,
                    } => {
                        assert!(*remaining > 0);
                        assert!(*decays_at > 0);
                        println!("throttle-redis:{remaining}");
                    }
                }
                true
            })
            .deleteWhen(move |error| matches_error(error, "_discard"))
            .failWhen(move |error| matches_error(error, "_terminal"));
        let middleware: Arc<dyn JobMiddleware> = if self.0.starts_with("throttle_redis_") {
            Arc::new(ThrottlesExceptionsWithRedis::from(throttle).connection("default"))
        } else {
            Arc::new(throttle)
        };
        vec![middleware, Arc::new(Tag)]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            Handler.handle(context, payload).await?;
            if self.0.ends_with("_success") {
                return Ok(());
            }
            Err(execution(std::io::Error::other(self.0)))
        })
    }
}

struct ConditionalHandler {
    name: &'static str,
    gate: PathBuf,
}

impl JobHandler for ConditionalHandler {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        let condition = || {
            println!("conditional-evaluation:{}", self.name);
            self.gate.exists()
        };
        let middleware: Arc<dyn JobMiddleware> = match self.name {
            "conditional_skip_when" => Arc::new(Skip::when(true)),
            "conditional_skip_unless" => Arc::new(Skip::unless(condition)),
            "conditional_run_when" => Arc::new(Skip::when(condition)),
            "conditional_run_unless" => Arc::new(Release::unless(true, Duration::from_secs(2))),
            "conditional_release_zero" => Arc::new(Release::when(|| !condition(), Duration::ZERO)),
            "conditional_release_delayed" => {
                Arc::new(Release::unless(condition, Duration::from_secs(2)))
            }
            _ => unreachable!(),
        };
        vec![middleware, Arc::new(Tag)]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Handler.handle(context, payload)
    }
}

struct OverlapHandler {
    name: &'static str,
    gate: PathBuf,
}

impl JobHandler for OverlapHandler {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        let base = WithoutOverlapping::new("resource");
        let shared = || {
            WithoutOverlapping::new("resource")
                .shared()
                .withPrefix("fixture-overlap:")
        };
        let lease = || {
            WithoutOverlapping::new("lease")
                .shared()
                .withPrefix("fixture-overlap:")
        };
        let middleware = match self.name {
            "overlap_holder" | "overlap_after_error" | "overlap_error" => shared(),
            "overlap_deferred" => shared().releaseAfter(SystemTime::now() + Duration::from_secs(3)),
            "overlap_discard" => shared().dontRelease(),
            "overlap_isolated" => base.withPrefix("fixture-overlap:"),
            "overlap_other_prefix" => base.shared().withPrefix("fixture-other:"),
            "overlap_lease_holder" => {
                lease().expireAfter(SystemTime::now() + Duration::from_secs(2))
            }
            "overlap_lease_probe" => lease().dontRelease(),
            _ => unreachable!(),
        };
        vec![Arc::new(middleware), Arc::new(Tag)]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            Handler.handle(context, payload).await?;
            if matches!(self.name, "overlap_holder" | "overlap_lease_holder") {
                while !self.gate.exists() {
                    sleep(Duration::from_millis(20)).await;
                }
            }
            if self.name == "overlap_error" {
                return Err(execution(std::io::Error::other("overlap fixture error")));
            }
            Ok(())
        })
    }
}

struct Providers;
impl ServiceProvider for Providers {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if std::env::var_os("QUEUE_WORKER_TEST_CUSTOM_DETECTOR").is_some() {
                app.container()
                    .instance::<dyn LostConnectionDetector>(Arc::new(FixtureLostConnectionDetector))
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            }
            if std::env::var_os("QUEUE_WORKER_DISABLE_POLLING").is_some() {
                Queue::withoutInterruptionPolling()?;
            }
            for name in [
                "flaky",
                "manual_release",
                "mysql_disconnect",
                "terminal",
                "timeout",
                "retry_timeout",
                "slow",
                "pause_gate",
                "restart_gate",
                "signal_gate",
                "control_default",
                "control_low",
            ] {
                Queue::register(name, Arc::new(Handler))?;
            }
            for name in ["interrupt_gate", "interrupt_timeout", "interrupt_error"] {
                Queue::register(name, Arc::new(InterruptibleHandler::default()))?;
            }
            for name in [
                "rate_alpha",
                "rate_beta",
                "rate_drop",
                "rate_unlimited",
                "rate_missing",
                "rate_redis",
            ] {
                Queue::register(name, Arc::new(LimitedHandler(name)))?;
            }
            for name in [
                "throttle_cache_retry",
                "throttle_cache_success",
                "throttle_cache_blocked",
                "throttle_cache_discard",
                "throttle_cache_terminal",
                "throttle_cache_bypass",
                "throttle_redis_retry",
                "throttle_redis_success",
                "throttle_redis_blocked",
                "throttle_redis_discard",
                "throttle_redis_terminal",
                "throttle_redis_bypass",
            ] {
                Queue::register(name, Arc::new(ThrottledHandler(name)))?;
            }
            for name in [
                "conditional_skip_when",
                "conditional_skip_unless",
                "conditional_run_when",
                "conditional_run_unless",
                "conditional_release_zero",
                "conditional_release_delayed",
            ] {
                Queue::register(
                    name,
                    Arc::new(ConditionalHandler {
                        name,
                        gate: app.base_path().join("conditional.ready"),
                    }),
                )?;
            }
            for name in [
                "overlap_holder",
                "overlap_deferred",
                "overlap_discard",
                "overlap_isolated",
                "overlap_other_prefix",
                "overlap_lease_holder",
                "overlap_lease_probe",
                "overlap_error",
                "overlap_after_error",
            ] {
                Queue::register(
                    name,
                    Arc::new(OverlapHandler {
                        name,
                        gate: app.base_path().join("overlap.ready"),
                    }),
                )?;
            }
            let cache_error = |source| HttpError::Internal {
                source: Box::new(source),
            };
            RateLimiter::r#for("job-budget", |_| Limit::perMinute(0)).map_err(cache_error)?;
            RateLimiter::forJob("job-budget", |job| {
                Limit::perSecond(1).by(job.data().as_str().unwrap())
            })
            .map_err(cache_error)?;
            RateLimiter::forJob("job-drop", |_| Limit::perMinute(0)).map_err(cache_error)?;
            RateLimiter::forJob("job-unlimited", |_| Limit::none()).map_err(cache_error)?;
            RateLimiter::forJob("job-redis", |job| {
                vec![
                    Limit::perMinute(1).by(job.data().as_str().unwrap()),
                    Limit::perMinute(2).by("all"),
                ]
            })
            .map_err(cache_error)?;
            Context::dehydrating(|context| context.forgetHidden("secret"))?;
            Context::hydrated(|context| context.add("hydrated", json!(true)))?;
            Ok(())
        })
    }
}

fn dispatch(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = request.route("kind").unwrap();
        let trace = if kind == "rollback" { "invoice" } else { kind };
        Context::add("trace", json!(trace)).unwrap();
        Context::addHidden("secret", json!("remove")).unwrap();
        Context::addHidden("protected", json!("hidden")).unwrap();
        if kind == "timed_pause" {
            let connection = Queue::connection(None)?;
            let name = connection.get_connection_name();
            Queue::pauseFor(name, "default", Ttl::Seconds(3)).await?;
            assert!(Queue::isPaused(name, "default").await?);
            assert_eq!(
                Queue::getPausedQueues(name, &["low", "default"]).await?,
                ["default"]
            );
        } else if matches!(kind, "invoice" | "rollback") {
            let mut transaction =
                DB::beginTransaction()
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            Event::dispatchInTransaction(Invoice(42), &mut transaction).await?;
            if kind == "rollback" {
                transaction.rollBack(None).await
            } else {
                transaction.commit().await
            }
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        } else {
            let mut payload = if kind.starts_with("rate_") {
                Payload::new(kind, kind)?
            } else {
                Payload::new(kind, ())?
            };
            if matches!(
                kind,
                "timeout" | "retry_timeout" | "interrupt_timeout" | "interrupt_error"
            ) {
                payload = payload.with_options(JobOptions {
                    timeout: Some(Duration::from_millis(
                        if matches!(kind, "timeout" | "retry_timeout") {
                            100
                        } else {
                            250
                        },
                    )),
                    fail_on_timeout: kind != "retry_timeout",
                    ..Default::default()
                });
            }
            let options = DispatchOptions {
                queue: if kind.starts_with("throttle_redis_") {
                    Some("throttle-redis".into())
                } else if kind.starts_with("throttle_cache_") {
                    Some("throttle-cache".into())
                } else if kind.starts_with("conditional_") {
                    Some("conditional".into())
                } else if kind.starts_with("overlap_") {
                    Some("overlap".into())
                } else {
                    (kind == "control_low").then(|| "low".to_owned())
                },
                ..Default::default()
            };
            Queue::dispatch(payload, options, None).await?;
        }
        assert_eq!(Context::getHidden("secret").unwrap(), Some(json!("remove")));
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/dispatch/{kind}", dispatch)?;
    Ok(router)
}

fn application(root: &Path) -> Application {
    Application::configure(root.into())
        .withConfig(|base, _| {
            let mut config: serde_json::Map<String, Json> =
                serde_json::from_slice(&std::fs::read(base.join("queue-fixture.json")).unwrap())
                    .unwrap();
            if std::env::var_os("QUEUE_WORKER_POP_EVENT").is_some() {
                for driver in ["database", "redis"] {
                    config.get_mut("queue").unwrap()["connections"][driver]["retry_after"] =
                        json!(2);
                }
            }
            if let Some(budget) = std::env::var("QUEUE_WORKER_TEST_MEMORY_BUDGET")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
            {
                config.get_mut("queue").unwrap()["memory_limit"] = json!(budget);
            }
            if let Ok(port) = std::env::var("QUEUE_WORKER_TEST_REDIS_PROXY") {
                config.get_mut("database").unwrap()["redis"]["default"]["port"] =
                    json!(port.parse::<u16>().unwrap());
            }
            if std::env::var_os("QUEUE_WORKER_TEST_CONTINUE_ON_DISCONNECT").is_some() {
                config.get_mut("queue").unwrap()["stop_on_lost_connection"] = json!(false);
            }
            if std::env::var_os("QUEUE_WORKER_TEST_SHORT_LEASE").is_some() {
                for driver in ["database", "redis"] {
                    config.get_mut("queue").unwrap()["connections"][driver]["retry_after"] =
                        json!(2);
                }
            }
            if std::env::var_os("QUEUE_WORKER_TEST_KEEP_ON_TIMEOUT").is_some() {
                config.get_mut("queue").unwrap()["kill_on_timeout"] = json!(false);
            }
            if let Ok(code) = std::env::var("QUEUE_WORKER_TEST_TIMEOUT_EXIT_CODE") {
                config.get_mut("queue").unwrap()["timed_out_exit_code"] =
                    json!(code.parse::<u8>().unwrap());
            }
            Ok(config.into_iter().collect())
        })
        .withProviders(vec![Box::new(Providers)])
        .withEvents(DiscoverEvents::new(&[listeners]))
        .withExceptions(|exceptions| {
            exceptions.report::<sqlx::Error>(|_| {
                println!("lost-report:mysql");
                ReportDecision::Continue
            });
            exceptions.report::<::redis::RedisError>(|_| {
                println!("lost-report:redis");
                ReportDecision::Continue
            });
            exceptions.report::<std::io::Error>(|error| {
                if error.to_string() == "pop-event:after-error" {
                    println!("pop-event:reported");
                    ReportDecision::Stop
                } else if error.to_string() == "interrupt-hook-error" {
                    println!("job-interruption:reported");
                    ReportDecision::Stop
                } else if error.to_string().starts_with("throttle_") {
                    println!("throttle-report:{error}");
                    ReportDecision::Stop
                } else if error.to_string().starts_with("attempt-event:") {
                    println!("attempt-report:{error}");
                    ReportDecision::Stop
                } else {
                    ReportDecision::Continue
                }
            });
        })
        .withRouting(Routing::new(routes))
        .create()
        .unwrap()
}

#[tokio::test]
#[ignore = "Spawned by the worker integration test"]
async fn worker_artisan_fixture() {
    let root = PathBuf::from(std::env::var_os("QUEUE_WORKER_FIXTURE").unwrap());
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("QUEUE_WORKER_ARGS").unwrap()).unwrap();
    let args: Vec<_> = args.into_iter().map(OsString::from).collect();
    let expected = std::env::var("QUEUE_WORKER_EXPECT_EXIT")
        .ok()
        .map_or(0, |status| status.parse().unwrap());
    let mut app = application(&root);
    let fake = if std::env::var_os("QUEUE_WORKER_FAKE_POP").is_some() {
        app.boot().await.unwrap();
        Some(
            app.make::<Dispatcher>()
                .await
                .unwrap()
                .fake(&[JobPopping::NAME, JobPopped::NAME]),
        )
    } else {
        None
    };
    let status = app.handleCommand(&args).await.unwrap();
    assert_eq!(status, expected);
    if std::env::var_os("QUEUE_WORKER_TEST_REAL_EXIT").is_some() {
        std::process::exit(status);
    }
    if let Some(fake) = fake {
        fake.assertDispatchedTimes(JobPopping::NAME, 1)
            .assertDispatchedTimes(JobPopped::NAME, 1);
        println!("pop-event:fake");
    }
}

fn artisan(root: &Path, args: &[&str]) -> Command {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "worker_artisan_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("QUEUE_WORKER_FIXTURE", root)
        .env("QUEUE_WORKER_ARGS", serde_json::to_string(args).unwrap())
        .kill_on_drop(true);
    child
}

async fn run(root: &Path, args: &[&str]) -> String {
    let output = timeout(Duration::from_secs(12), artisan(root, args).output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn json_lines(output: &str) -> Vec<Json> {
    output
        .lines()
        .filter_map(|line| line.find('{').map(|offset| &line[offset..]))
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn captured_worker_output(worker: &mut tokio::process::Child) -> Vec<Json> {
    let mut stdout = String::new();
    timeout(
        Duration::from_secs(2),
        worker.stdout.take().unwrap().read_to_string(&mut stdout),
    )
    .await
    .unwrap()
    .unwrap();
    json_lines(&stdout)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(address: SocketAddr, kind: &str) {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("GET /dispatch/{kind} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 204 "), "{response}");
    }).await.unwrap();
}

struct TempRoot(PathBuf);
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn exercise(redis: Option<u16>, mysql: Option<&str>) {
    let id = format!("{:016x}", fastrand::u64(..));
    let root = TempRoot(std::env::temp_dir().join(format!("openrust-worker-{id}")));
    std::fs::create_dir_all(&root.0).unwrap();
    std::fs::File::create(root.0.join("fixture.sqlite")).unwrap();
    let default_queue = if redis.is_some() { "redis" } else { "database" };
    let db_config = mysql.map_or_else(
        || json!({"driver":"sqlite", "database":"fixture.sqlite"}),
        |url| json!({"url":url}),
    );
    let config = json!({
        "database": {
            "default": "worker", "connections": {"worker":db_config},
            "redis": {"options":{"prefix":format!("openrust-worker:{id}:")}, "default":{"port":redis.unwrap_or(6379)}}
        },
        "cache": {"default":if redis.is_some() {"redis"} else {"file"},"stores":{"redis":{"driver":"redis"},"file":{"driver":"file","path":"control-cache"}}},
        "queue": {
            "default": default_queue, "output_timezone": "Asia/Shanghai",
            "connections":{"database":{"driver":"database","retry_after":30},"redis":{"driver":"redis","retry_after":30,"block_for":0}},
            "failed":{"driver":"database-uuids","database":"worker","table":"failed_jobs"}
        }
    });
    std::fs::write(root.0.join("queue-fixture.json"), config.to_string()).unwrap();
    let mut app = application(&root.0);
    app.boot().await.unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let manager = app.container().make::<QueueManager>().unwrap();
    let queue = manager.connection(None).unwrap();
    let mut schema = db.getSchemaBuilder();
    for table in ["receipts", "jobs", "failed_jobs"] {
        schema.dropIfExists(table).await.unwrap();
    }
    schema
        .create("receipts", |table| {
            table.string("name", 255);
            table.bigInteger("value");
        })
        .await
        .unwrap();
    schema
        .create("jobs", |table| {
            table.id();
            table.string("queue", 255).index();
            table.longText("payload");
            table.unsignedInteger("attempts");
            table.unsignedBigInteger("reserved_at").nullable();
            table.unsignedBigInteger("available_at");
            table.unsignedBigInteger("created_at");
        })
        .await
        .unwrap();
    schema
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", 255).unique();
            table.text("connection");
            table.text("queue");
            table.longText("payload");
            table.longText("exception");
            table.timestamp("failed_at", 0);
        })
        .await
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    request(address, "flaky").await;
    let output = run(&root.0, &["queue:work", "--json", "--once", "--tries=2"]).await;
    let rows = json_lines(&output);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["status"], "starting");
    assert_eq!(rows[1]["status"], "released_after_exception");
    assert_eq!(rows[1]["result"], "released");
    assert_eq!(rows[1]["exception"], "QueueError");
    assert_eq!(rows[1]["attempts"], 1);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    let output = run(&root.0, &["queue:work", "--json", "--once", "--tries=2"]).await;
    let rows = json_lines(&output);
    assert_eq!(rows[1]["status"], "success");
    assert_eq!(rows[1]["attempts"], 2);
    assert_eq!(queue.size(None).await.unwrap(), 0);
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "flaky")
            .count("*")
            .await
            .unwrap(),
        2
    );

    request(address, "manual_release").await;
    let output = run(&root.0, &["queue:work", "--json", "--once", "--tries=2"]).await;
    let rows = json_lines(&output);
    assert_eq!(rows[1]["status"], "success");
    assert_eq!(rows[1]["result"], "released");
    assert!(rows[1].get("exception").is_none());
    run(&root.0, &["queue:work", "--quiet", "--once", "--tries=2"]).await;
    assert_eq!(queue.size(None).await.unwrap(), 0);

    request(address, "rollback").await;
    assert_eq!(queue.size(None).await.unwrap(), 0);
    request(address, "invoice").await;
    let output = run(&root.0, &["queue:work", "--json", "--stop-when-empty"]).await;
    let rows = json_lines(&output);
    let stop = rows.last().unwrap();
    assert_eq!(stop["reason"], "empty");
    assert_eq!(stop["jobs_processed"], 1);
    assert_eq!(stop["exit_code"], 0);
    assert_eq!(stop["connection"], default_queue);
    assert_eq!(stop["queue"], "default");
    assert!(stop["timestamp"].as_str().unwrap().ends_with("+08:00"));
    assert_eq!(
        db.scalar("select value from receipts where name = 'invoice'", &[])
            .await
            .unwrap(),
        Value::Integer(42)
    );

    request(address, "terminal").await;
    request(address, "timeout").await;
    let args = ["queue:work", "--json", "--stop-when-empty"];
    let output = timeout(
        Duration::from_secs(8),
        artisan(&root.0, &args)
            .env("QUEUE_WORKER_EXPECT_EXIT", "1")
            .env("QUEUE_WORKER_TEST_REAL_EXIT", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let output = String::from_utf8(output.stdout).unwrap();
    let rows = json_lines(&output);
    assert!(
        rows.iter()
            .any(|row| row["job"] == "terminal" && row["status"] == "failed")
    );
    assert_eq!(queue.size(None).await.unwrap(), 0);
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 2);
    assert_eq!(rows.last().unwrap()["reason"], "timed_out");
    assert_eq!(rows.last().unwrap()["exit_code"], 1);
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "late")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.scalar(
            r#"select exception from failed_jobs where payload like '%"name":"timeout"%'"#,
            &[]
        )
        .await
        .unwrap(),
        Value::Text("queued job timed out".into())
    );
    let listing = run(&root.0, &["queue:failed", "--json"]).await;
    let data: Vec<Json> =
        serde_json::from_str(listing.lines().find(|line| line.starts_with("[{")).unwrap()).unwrap();
    let terminal = data.iter().find(|job| job["name"] == "terminal").unwrap()["id"]
        .as_str()
        .unwrap();
    db.statement(
        "insert into receipts (name, value) values ('allow', 1)",
        &[],
    )
    .await
    .unwrap();
    run(&root.0, &["queue:retry", terminal]).await;
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 1);
    run(&root.0, &["queue:work", "--once"]).await;
    assert_eq!(queue.size(None).await.unwrap(), 0);
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "terminal")
            .r#where("value", "=", 1_i64)
            .count("*")
            .await
            .unwrap(),
        2
    );
    run(&root.0, &["queue:flush"]).await;
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 0);

    #[cfg(unix)]
    {
        request(address, "slow").await;
        let mut worker = artisan(&root.0, &["queue:work", "--json", "--sleep=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        timeout(Duration::from_secs(5), async {
            while db
                .table("receipts")
                .r#where("name", "=", "slow")
                .count("*")
                .await
                .unwrap()
                == 0
            {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            Command::new("kill")
                .arg("-TERM")
                .arg(worker.id().unwrap().to_string())
                .status()
                .await
                .unwrap()
                .success()
        );
        assert!(
            timeout(Duration::from_secs(5), worker.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert_eq!(
            db.table("receipts")
                .r#where("name", "=", "slow_done")
                .count("*")
                .await
                .unwrap(),
            1
        );
        assert_eq!(queue.size(None).await.unwrap(), 0);
        let rows = captured_worker_output(&mut worker).await;
        assert_eq!(rows.last().unwrap()["reason"], "interrupted");
        assert_eq!(rows.last().unwrap()["jobs_processed"], 1);
    }
    if redis.is_none() && mysql.is_none() {
        let separate = TempRoot(root.0.join("separate-app"));
        std::fs::create_dir_all(&separate.0).unwrap();
        std::fs::write(separate.0.join("queue-fixture.json"), config.to_string()).unwrap();
        let mut other = application(&separate.0);
        other.boot().await.unwrap();
        let dispatcher = other.container().make::<Dispatcher>().unwrap();
        let events = dispatcher.fake(&[QueuePaused::NAME, QueueResumed::NAME]);
        let other_manager = other.container().make::<QueueManager>().unwrap();
        other_manager
            .pauseFor(
                "database",
                "default",
                Ttl::Until(SystemTime::now() + Duration::from_secs(30)),
            )
            .await
            .unwrap();
        assert!(other_manager.isPaused("database", "default").await.unwrap());
        assert!(!manager.isPaused("database", "default").await.unwrap());
        other_manager.resume("database", "default").await.unwrap();
        assert!(!other_manager.isPaused("database", "default").await.unwrap());
        assert_eq!(events.dispatched(QueuePaused::NAME).len(), 1);
        assert_eq!(events.dispatched(QueueResumed::NAME).len(), 1);
    }
    verify_worker_control(&root.0, &db, &*queue, address, default_queue).await;
    verify_worker_cadence(&root.0, &db, address, redis.is_some()).await;
    verify_worker_lifecycle(&root.0, &db, &*queue, address).await;
    verify_job_pop_events(&root.0, &db, &*queue, address).await;
    verify_job_attempt_events(&root.0, &db, &*queue, address).await;
    #[cfg(unix)]
    verify_worker_signals(&root.0, &db, &*queue, address).await;
    verify_job_interruption(&root.0, &db, address).await;
    verify_worker_memory(&root.0, &*queue, address).await;
    verify_worker_timeout(&root.0, &*queue, address).await;
    verify_job_rate_limits(&root.0, &db, &*queue, address, redis.is_some()).await;
    verify_conditional_middleware(&root.0, &db, &*queue, address).await;
    verify_job_overlap(&root.0, &db, &*queue, address).await;
    verify_exception_throttle(&root.0, &db, &*queue, address, "cache").await;
    if redis.is_some() {
        verify_exception_throttle(&root.0, &db, &*queue, address, "redis").await;
    }
    if mysql.is_none() && redis.is_none() {
        verify_custom_disconnect(&root.0, &*queue, address).await;
    }
    if mysql.is_some() {
        verify_mysql_disconnect(&root.0, &db, &*queue, address).await;
    }
    if let Some(port) = redis {
        verify_redis_disconnect(&root.0, port, address).await;
    }
    server.abort();
    for table in ["receipts", "jobs", "failed_jobs"] {
        schema.dropIfExists(table).await.unwrap();
    }
}

async fn verify_worker_control(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
    connection: &str,
) {
    let selected = format!("{connection}:default");
    let count = |name: &'static str| async move {
        db.table("receipts")
            .r#where("name", "=", name)
            .count("*")
            .await
            .unwrap()
    };
    let wait_for = |name| async move {
        timeout(Duration::from_secs(6), async {
            while count(name).await == 0 {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    };
    let release = |name: &'static str| async move {
        db.statement(
            "insert into receipts (name, value) values (?, 1)",
            &[name.into()],
        )
        .await
        .unwrap();
    };
    let once = ["queue:work", "--once"];
    run(root, &["queue:pause", &selected]).await;
    request(address, "control_default").await;
    request(address, "control_low").await;
    run(root, &["queue:pause", "--all"]).await;
    run(root, &["queue:work", "--once", "--queue=default,low"]).await;
    assert_eq!(count("control_low").await, 0);
    run(root, &["queue:resume", "--all"]).await;
    run(root, &["queue:work", "--once", "--queue=default,low"]).await;
    assert_eq!(count("control_low").await, 1);
    assert_eq!(count("control_default").await, 0);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    run(root, &["queue:continue", "default"]).await;
    run(root, &once).await;
    assert_eq!(count("control_default").await, 1);

    request(address, "pause_gate").await;
    request(address, "control_default").await;
    let mut worker = artisan(root, &["queue:work", "--json", "--sleep=1", "--max-jobs=2"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for("pause_gate").await;
    run(root, &["queue:pause", &selected]).await;
    release("pause_gate_release").await;
    wait_for("pause_gate_done").await;
    assert!(
        timeout(Duration::from_millis(1200), worker.wait())
            .await
            .is_err()
    );
    assert_eq!(count("control_default").await, 1);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    run(root, &["queue:resume", &selected]).await;
    assert!(
        timeout(Duration::from_secs(6), worker.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let rows = captured_worker_output(&mut worker).await;
    assert!(rows.iter().any(|row| row["status"] == "paused"));
    assert!(rows.iter().any(|row| row["status"] == "resumed"));
    assert_eq!(rows.last().unwrap()["reason"], "max_jobs");
    assert_eq!(count("control_default").await, 2);

    request(address, "restart_gate").await;
    request(address, "control_default").await;
    let mut worker = artisan(root, &["queue:work", "--json", "--sleep=1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for("restart_gate").await;
    let broadcast = run(root, &["queue:restart"]).await;
    assert!(broadcast.contains("Broadcasting queue restart signal."));
    release("restart_gate_release").await;
    assert!(
        timeout(Duration::from_secs(6), worker.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let rows = captured_worker_output(&mut worker).await;
    assert_eq!(rows.last().unwrap()["reason"], "restart_signal");
    assert_eq!(rows.last().unwrap()["jobs_processed"], 1);
    assert_eq!(
        db.scalar(
            "select value from receipts where name = ?",
            &[format!("stop:{:?}", WorkerStopReason::ReceivedRestartSignal).into()]
        )
        .await
        .unwrap(),
        Value::Integer(1)
    );
    assert_eq!(count("restart_gate_done").await, 1);
    assert_eq!(count("control_default").await, 2);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    run(root, &once).await;
    assert_eq!(count("control_default").await, 3);
    assert_eq!(queue.size(None).await.unwrap(), 0);
    run(root, &["queue:pause", &selected]).await;
    request(address, "control_default").await;
    let result = timeout(
        Duration::from_secs(6),
        artisan(root, &once)
            .env("QUEUE_WORKER_DISABLE_POLLING", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(count("control_default").await, 4);
    request(address, "control_default").await;
    run(root, &once).await;
    assert_eq!(count("control_default").await, 4);
    run(root, &["queue:resume", &selected]).await;
    run(root, &once).await;
    assert_eq!(count("control_default").await, 5);
    assert_eq!(count(QueuesPaused::NAME).await, 1);
    assert_eq!(count(QueuesResumed::NAME).await, 1);
    assert!(count(QueuePaused::NAME).await >= 3);
    assert!(count(QueueResumed::NAME).await >= 3);
    assert!(count(WorkerQueuePaused::NAME).await >= 1);
    assert!(count(WorkerQueueResumed::NAME).await >= 1);

    let paused = count(WorkerQueuePaused::NAME).await;
    let resumed = count(WorkerQueueResumed::NAME).await;
    request(address, "timed_pause").await;
    request(address, "control_default").await;
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", QueuePaused::NAME)
            .r#where("value", "=", 3)
            .count("*")
            .await
            .unwrap(),
        1
    );
    let mut worker = artisan(root, &["queue:work", "--sleep=1", "--max-jobs=1"])
        .spawn()
        .unwrap();
    timeout(Duration::from_secs(2), async {
        while count(WorkerQueuePaused::NAME).await == paused {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(count("control_default").await, 5);
    assert!(
        timeout(Duration::from_secs(7), worker.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_eq!(count(WorkerQueuePaused::NAME).await, paused + 1);
    assert_eq!(count(WorkerQueueResumed::NAME).await, resumed + 1);
    assert_eq!(count("control_default").await, 6);
    let output = run(
        root,
        &[
            "queue:work",
            "--json",
            "--max-time=1",
            "--stop-when-empty-for=0",
        ],
    )
    .await;
    assert_eq!(json_lines(&output).last().unwrap()["reason"], "max_time");
    let output = run(
        root,
        &["queue:work", "--json", "--quiet", "--stop-when-empty"],
    )
    .await;
    assert!(json_lines(&output).is_empty());
    let output = run(root, &["queue:work", "--stop-when-empty"]).await;
    assert!(output.contains("Worker STOPPED Queue empty"));
}

async fn verify_worker_cadence(
    root: &Path,
    db: &openrust::database::Connection,
    address: SocketAddr,
    redis: bool,
) {
    request(address, "control_default").await;
    request(address, "control_default").await;
    let output = run(
        root,
        &[
            "queue:work",
            "--json",
            "--rest=1",
            "--stop-when-empty-for=2",
            "--sleep=30",
        ],
    )
    .await;
    let rows = json_lines(&output);
    assert_eq!(rows.len(), 5, "{output}");
    assert_eq!(rows[0]["status"], "starting");
    assert_eq!(rows[1]["status"], "success");
    assert_eq!(rows[2]["status"], "starting");
    assert_eq!(rows[3]["status"], "success");
    assert_eq!(rows[4]["reason"], "empty_for");
    assert_eq!(rows[4]["jobs_processed"], 2);
    let timestamp = |row: &Json| {
        chrono::DateTime::parse_from_rfc3339(row["timestamp"].as_str().unwrap()).unwrap()
    };
    assert!((timestamp(&rows[2]) - timestamp(&rows[1])).num_milliseconds() >= 950);
    assert!((timestamp(&rows[4]) - timestamp(&rows[3])).num_milliseconds() >= 1950);
    assert_eq!(
        db.scalar(
            "select value from receipts where name = 'stop:QueueEmptyFor'",
            &[]
        )
        .await
        .unwrap(),
        Value::Integer(2)
    );

    let output = run(
        root,
        &[
            "queue:work",
            "--json",
            "--stop-when-empty",
            "--stop-when-empty-for=30",
        ],
    )
    .await;
    assert_eq!(json_lines(&output).last().unwrap()["reason"], "empty");

    if redis {
        let start = tokio::time::Instant::now();
        let mut worker = artisan(
            root,
            &[
                "queue:work",
                "--json",
                "--stop-when-empty-for=5",
                "--sleep=30",
                "--max-jobs=1",
            ],
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
        sleep(Duration::from_millis(400)).await;
        request(address, "control_default").await;
        assert!(
            timeout(Duration::from_secs(3), worker.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(start.elapsed() < Duration::from_secs(4));
        let rows = captured_worker_output(&mut worker).await;
        assert_eq!(rows.last().unwrap()["reason"], "max_jobs");
        assert_eq!(rows.last().unwrap()["jobs_processed"], 1);
    }
}

async fn verify_worker_lifecycle(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let count = |name: &'static str| async move {
        db.table("receipts")
            .r#where("name", "=", name)
            .count("*")
            .await
            .unwrap()
    };
    let launch = |args: &[&str], veto: &'static str| {
        let mut command = artisan(root, args);
        command.env("QUEUE_WORKER_VETO", veto);
        async move {
            let output = timeout(Duration::from_secs(6), command.output())
                .await
                .unwrap()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            json_lines(&String::from_utf8(output.stdout).unwrap())
        }
    };
    let starting = count(WorkerStarting::NAME).await;
    let idle = count(WorkerIdle::NAME).await;
    let stopped = count("stop:MaxJobsExceeded").await;
    request(address, "control_default").await;
    let start = tokio::time::Instant::now();
    let rows = launch(
        &["queue:work", "--json", "--max-jobs=1", "--sleep=0"],
        "once",
    )
    .await;
    assert!(start.elapsed() >= Duration::from_millis(950));
    assert_eq!(rows.last().unwrap()["jobs_processed"], 1);
    assert_eq!(count("loop:veto").await, 1);
    assert_eq!(count("loop:allowed").await, 1);
    assert_eq!(count("loop:later").await, 1);
    assert_eq!(count(WorkerStarting::NAME).await, starting + 1);
    assert_eq!(count(WorkerIdle::NAME).await, idle);
    assert_eq!(count("stop:MaxJobsExceeded").await, stopped + 1);

    request(address, "control_default").await;
    let rows = launch(
        &["queue:work", "--json", "--max-time=1", "--sleep=0"],
        "always",
    )
    .await;
    assert_eq!(rows.last().unwrap()["reason"], "max_time");
    assert_eq!(rows.last().unwrap()["jobs_processed"], 0);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    assert_eq!(count("loop:later").await, 1);
    assert_eq!(count(WorkerIdle::NAME).await, idle);

    let veto_count = count("loop:veto").await;
    let rows = launch(&["queue:work", "--json", "--once"], "always").await;
    assert_eq!(rows.last().unwrap()["status"], "success");
    assert_eq!(queue.size(None).await.unwrap(), 0);
    assert_eq!(count("loop:veto").await, veto_count);
    assert_eq!(count(WorkerStarting::NAME).await, starting + 2);
    assert_eq!(count("stop:MaxJobsExceeded").await, stopped + 1);

    let rows = launch(&["queue:work", "--json", "--stop-when-empty"], "allow").await;
    assert_eq!(rows.last().unwrap()["reason"], "empty");
    assert_eq!(count(WorkerIdle::NAME).await, idle + 1);
    assert_eq!(count(WorkerStarting::NAME).await, starting + 3);

    request(address, "control_default").await;
    let veto_count = count("loop:veto").await;
    let mut worker = artisan(
        root,
        &["queue:work", "--json", "--stop-when-empty", "--sleep=3"],
    )
    .env("QUEUE_WORKER_VETO", "always")
    .stdout(Stdio::piped())
    .spawn()
    .unwrap();
    timeout(Duration::from_secs(5), async {
        while count("loop:veto").await == veto_count {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    run(root, &["queue:restart"]).await;
    assert!(
        timeout(Duration::from_secs(6), worker.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let rows = captured_worker_output(&mut worker).await;
    assert_eq!(rows.last().unwrap()["reason"], "restart_signal");
    assert_eq!(queue.size(None).await.unwrap(), 1);
    launch(&["queue:work", "--json", "--once"], "always").await;
    assert_eq!(queue.size(None).await.unwrap(), 0);
}

async fn verify_job_pop_events(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let processed = || async {
        db.table("receipts")
            .r#where("name", "=", "control_default")
            .count("*")
            .await
            .unwrap()
    };
    let launch = |mode: &'static str, status: &'static str| async move {
        let output = timeout(
            Duration::from_secs(8),
            artisan(root, &["queue:work", "--json", "--once", "--tries=2"])
                .env("QUEUE_WORKER_POP_EVENT", mode)
                .env("QUEUE_WORKER_EXPECT_EXIT", status)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let count = processed().await;
    let empty = launch("normal", "0").await;
    assert!(empty.contains("pop-event:before"));
    assert!(!empty.contains("pop-event:after"));

    request(address, "control_default").await;
    let before = launch("before-error", "1").await;
    assert!(before.contains("pop-event:before"));
    assert!(!before.contains("pop-event:after"));
    assert_eq!(queue.size(None).await.unwrap(), 1);
    assert_eq!(processed().await, count);

    let after = launch("after-error", "0").await;
    assert!(after.find("pop-event:before").unwrap() < after.find("pop-event:after:1").unwrap());
    assert!(after.contains("pop-event:reported"));
    assert!(json_lines(&after).is_empty());
    assert_eq!(processed().await, count);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    assert!(queue.pop_now(None).await.unwrap().is_none());

    sleep(Duration::from_secs(2)).await;
    let retry = launch("retry", "0").await;
    assert!(
        retry.find("pop-event:after:2").unwrap() < retry.find(r#""status":"starting""#).unwrap()
    );
    assert_eq!(json_lines(&retry).last().unwrap()["status"], "success");
    assert_eq!(processed().await, count + 1);
    assert_eq!(queue.size(None).await.unwrap(), 0);

    request(address, "control_default").await;
    let output = timeout(
        Duration::from_secs(8),
        artisan(root, &["queue:work", "--once"])
            .env("QUEUE_WORKER_FAKE_POP", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("pop-event:fake")
    );
    assert_eq!(processed().await, count + 2);
}

async fn verify_job_attempt_events(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let launch = |kind: &'static str, attempt, mode: &'static str, tries: &'static str| async move {
        let output = timeout(
            Duration::from_secs(8),
            artisan(root, &["queue:work", "--json", "--once", tries])
                .env("QUEUE_WORKER_ATTEMPT_EVENTS", mode)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        if mode.ends_with("-error") {
            let report = format!("attempt-report:attempt-event:{mode}");
            assert!(stdout.contains(&report), "{stdout}");
        }
        let suffix = format!(":{kind}:{attempt}");
        stdout
            .lines()
            .filter_map(|line| {
                line.strip_prefix("attempt-event:")?
                    .strip_suffix(&suffix)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>()
    };
    request(address, "manual_release").await;
    assert_eq!(
        launch("manual_release", 1, "normal", "--tries=2").await,
        [
            "JobProcessing",
            "JobProcessed",
            "JobReleased",
            "JobAttempted"
        ]
    );
    assert_eq!(
        launch("manual_release", 2, "normal", "--tries=2").await,
        ["JobProcessing", "JobProcessed", "JobAttempted"]
    );

    request(address, "flaky").await;
    assert_eq!(
        launch("flaky", 1, "normal", "--tries=2").await,
        [
            "JobProcessing",
            "JobExceptionOccurred",
            "JobReleasedAfterException",
            "JobAttempted"
        ]
    );
    assert_eq!(
        launch("flaky", 2, "normal", "--tries=2").await,
        ["JobProcessing", "JobProcessed", "JobAttempted"]
    );

    request(address, "flaky").await;
    assert_eq!(
        launch("flaky", 1, "normal", "--tries=1").await,
        [
            "JobProcessing",
            "JobFailed",
            "JobExceptionOccurred",
            "JobAttempted"
        ]
    );

    request(address, "timeout").await;
    assert_eq!(
        launch("timeout", 1, "normal", "--tries=2").await,
        [
            "JobProcessing",
            "JobFailed",
            "JobTimedOut",
            "JobExceptionOccurred",
            "JobAttempted"
        ]
    );

    let count = || async {
        db.table("receipts")
            .r#where("name", "=", "control_default")
            .count("*")
            .await
            .unwrap()
    };
    let before = count().await;
    request(address, "control_default").await;
    assert_eq!(
        launch("control_default", 1, "preflight-error", "--tries=2").await,
        [
            "JobProcessing",
            "JobExceptionOccurred",
            "JobReleasedAfterException",
            "JobAttempted"
        ]
    );
    assert_eq!(count().await, before);
    assert_eq!(
        launch("control_default", 2, "normal", "--tries=2").await,
        ["JobProcessing", "JobProcessed", "JobAttempted"]
    );
    assert_eq!(count().await, before + 1);

    request(address, "control_default").await;
    assert_eq!(
        launch("control_default", 1, "processed-error", "--tries=2").await,
        ["JobProcessing", "JobProcessed", "JobAttempted"],
    );
    assert_eq!(count().await, before + 2);
    assert_eq!(queue.size(None).await.unwrap(), 0);
}

#[cfg(unix)]
async fn verify_worker_signals(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    async fn wait_for(db: &openrust::database::Connection, name: &str) {
        timeout(Duration::from_secs(5), async {
            while db
                .table("receipts")
                .r#where("name", "=", name)
                .count("*")
                .await
                .unwrap()
                == 0
            {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("missing receipt {name}"));
    }
    fn send(worker: &tokio::process::Child, signal: rustix::process::Signal) {
        let pid = rustix::process::Pid::from_raw(worker.id().unwrap().try_into().unwrap()).unwrap();
        rustix::process::kill_process(pid, signal).unwrap();
    }

    use rustix::process::Signal;
    request(address, "signal_gate").await;
    let mut worker = artisan(root, &["queue:work", "--json", "--sleep=1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for(db, "signal_gate").await;
    send(&worker, Signal::USR2);
    wait_for(db, WorkerPausing::NAME).await;
    request(address, "control_default").await;
    db.statement(
        "insert into receipts (name, value) values ('signal_gate_release', 1)",
        &[],
    )
    .await
    .unwrap();
    wait_for(db, "signal_gate_done").await;
    sleep(Duration::from_millis(250)).await;
    assert_eq!(queue.size(None).await.unwrap(), 1);
    send(&worker, Signal::CONT);
    wait_for(db, WorkerResuming::NAME).await;
    timeout(Duration::from_secs(5), async {
        while queue.size(None).await.unwrap() > 0 {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    send(&worker, Signal::QUIT);
    assert!(
        timeout(Duration::from_secs(5), worker.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let rows = captured_worker_output(&mut worker).await;
    assert_eq!(rows.last().unwrap()["reason"], "interrupted");
    assert_eq!(rows.last().unwrap()["jobs_processed"], 2);
    let starts = || async {
        db.table("receipts")
            .r#where("name", "=", WorkerStarting::NAME)
            .count("*")
            .await
            .unwrap()
    };
    let before = starts().await;
    let mut idle = artisan(root, &["queue:work", "--json", "--sleep=1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    timeout(Duration::from_secs(5), async {
        while starts().await == before {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    send(&idle, Signal::INT);
    assert!(
        timeout(Duration::from_secs(5), idle.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let rows = captured_worker_output(&mut idle).await;
    assert_eq!(rows.last().unwrap()["reason"], "interrupted");
    assert_eq!(rows.last().unwrap()["jobs_processed"], 0);

    for signal in [Signal::TERM, Signal::QUIT, Signal::INT] {
        assert_eq!(
            db.table("receipts")
                .r#where("name", "=", WorkerInterrupted::NAME)
                .r#where("value", "=", i64::from(signal.as_raw()))
                .count("*")
                .await
                .unwrap(),
            1,
        );
    }
}

async fn verify_job_interruption(
    root: &Path,
    db: &openrust::database::Connection,
    address: SocketAddr,
) {
    #[cfg(unix)]
    {
        use rustix::process::{Pid, Signal, kill_process};
        request(address, "interrupt_gate").await;
        let mut worker = artisan(root, &["queue:work", "--json", "--sleep=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        timeout(Duration::from_secs(5), async {
            while db
                .table("receipts")
                .r#where("name", "=", "interrupt_gate")
                .count("*")
                .await
                .unwrap()
                == 0
            {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let pid = Pid::from_raw(worker.id().unwrap().try_into().unwrap()).unwrap();
        kill_process(pid, Signal::TERM).unwrap();
        assert!(
            timeout(Duration::from_secs(5), worker.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        let mut out = String::new();
        worker
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut out)
            .await
            .unwrap();
        let signal = Signal::TERM.as_raw();
        let callback = out
            .find(&format!(
                "job-interruption:callback:interrupt_gate:{signal}"
            ))
            .unwrap();
        let event = out
            .find(&format!("job-interruption:event:interrupt_gate:{signal}"))
            .unwrap();
        assert!(callback < event, "{out}");
        let rows = json_lines(&out);
        assert_eq!(rows[1]["status"], "success");
        assert_eq!(rows.last().unwrap()["reason"], "interrupted");
    }
    #[cfg(unix)]
    let alarm = rustix::process::Signal::ALARM.as_raw();
    #[cfg(not(unix))]
    let alarm = 14;
    let before = db.table("failed_jobs").count("*").await.unwrap();
    for kind in ["interrupt_timeout", "interrupt_error"] {
        request(address, kind).await;
        let out = run(root, &["queue:work", "--json", "--once", "--tries=2"]).await;
        assert!(
            out.contains(&format!("job-interruption:callback:{kind}:{alarm}")),
            "{out}"
        );
        assert_eq!(
            out.contains(&format!("job-interruption:event:{kind}:{alarm}")),
            kind == "interrupt_timeout",
            "{out}",
        );
        assert_eq!(
            out.contains("job-interruption:reported"),
            kind == "interrupt_error",
            "{out}"
        );
        assert_eq!(json_lines(&out)[1]["status"], "failed");
    }
    assert_eq!(
        db.table("failed_jobs").count("*").await.unwrap(),
        before + 2
    );
}

async fn verify_worker_memory(
    root: &Path,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let launch = |limit: &'static str, budget: Option<&'static str>, expected: &'static str| async move {
        let mut command = artisan(root, &["queue:work", "--json", "--stop-when-empty", limit]);
        command.env("QUEUE_WORKER_EXPECT_EXIT", expected);
        if let Some(budget) = budget {
            command.env("QUEUE_WORKER_TEST_MEMORY_BUDGET", budget);
        }
        let output = timeout(Duration::from_secs(8), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        format!(
            "{}{}",
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap()
        )
    };
    request(address, "control_default").await;
    request(address, "control_default").await;
    let output = launch("--memory=1", None, "12").await;
    let rows = json_lines(&output);
    let stop = rows.last().unwrap();
    assert_eq!(stop["reason"], "memory");
    assert_eq!(stop["exit_code"], 12);
    assert_eq!(stop["level"], "warning");
    assert_eq!(stop["jobs_processed"], 1);
    assert!(stop["memory"].as_f64().unwrap() >= 1.0);
    assert_eq!(queue.size(None).await.unwrap(), 1);

    let once = run(root, &["queue:work", "--json", "--once", "--memory=1"]).await;
    assert_eq!(json_lines(&once).last().unwrap()["status"], "success");
    assert_eq!(queue.size(None).await.unwrap(), 0);

    let output = launch("--memory=50%", Some("2"), "12").await;
    let rows = json_lines(&output);
    assert_eq!(rows.last().unwrap()["reason"], "memory");
    assert_eq!(rows.last().unwrap()["jobs_processed"], 0);

    let output = launch("--memory=50%", None, "1").await;
    assert!(output.contains("queue.memory_limit"), "{output}");
    let output = launch("--memory=NaN%", Some("2"), "1").await;
    assert!(output.contains("finite percentage"), "{output}");
    for limit in ["--memory=0", "--memory=-1"] {
        let output = launch(limit, None, "0").await;
        let rows = json_lines(&output);
        assert_eq!(rows.last().unwrap()["reason"], "empty");
        assert!(rows.last().unwrap()["memory"].as_f64().unwrap() > 0.0);
    }
}

async fn verify_worker_timeout(
    root: &Path,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let args = ["queue:work", "--json", "--stop-when-empty", "--tries=2"];
    request(address, "retry_timeout").await;
    request(address, "control_default").await;
    let output = timeout(
        Duration::from_secs(8),
        artisan(root, &args)
            .env("QUEUE_WORKER_TEST_SHORT_LEASE", "1")
            .env("QUEUE_WORKER_ATTEMPT_EVENTS", "observe")
            .env("QUEUE_WORKER_TEST_TIMEOUT_EXIT_CODE", "124")
            .env("QUEUE_WORKER_EXPECT_EXIT", "124")
            .env("QUEUE_WORKER_TEST_REAL_EXIT", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(output.status.code(), Some(124), "{output:?}");
    let out = String::from_utf8(output.stdout).unwrap();
    assert!(
        out.contains("attempt-event:JobTimedOut:retry_timeout:1"),
        "{out}"
    );
    assert!(
        !out.contains("attempt-event:JobAttempted:retry_timeout:1"),
        "{out}"
    );
    assert!(
        !out.contains("attempt-event:JobReleasedAfterException:retry_timeout:1"),
        "{out}"
    );
    let rows = json_lines(&out);
    assert_eq!(rows[1]["status"], "timed_out");
    assert!(rows[1].get("result").is_none());
    assert_eq!(rows.last().unwrap()["reason"], "timed_out");
    assert_eq!(rows.last().unwrap()["exit_code"], 124);
    assert_eq!(rows.last().unwrap()["jobs_processed"], 1);
    assert_eq!(queue.size(None).await.unwrap(), 2);
    let next = run(root, &["queue:work", "--json", "--once"]).await;
    assert_eq!(json_lines(&next).last().unwrap()["job"], "control_default");
    assert_eq!(queue.size(None).await.unwrap(), 1);

    sleep(Duration::from_secs(2)).await;
    let output = timeout(
        Duration::from_secs(8),
        artisan(root, &args)
            .env("QUEUE_WORKER_TEST_SHORT_LEASE", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    let out = String::from_utf8(output.stdout).unwrap();
    assert!(
        json_lines(&out)
            .iter()
            .any(|row| row["job"] == "retry_timeout"
                && row["attempts"] == 2
                && row["status"] == "success")
    );
    assert_eq!(queue.size(None).await.unwrap(), 0);

    request(address, "retry_timeout").await;
    let output = timeout(
        Duration::from_secs(8),
        artisan(root, &args)
            .env("QUEUE_WORKER_TEST_KEEP_ON_TIMEOUT", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    let out = String::from_utf8(output.stdout).unwrap();
    let rows = json_lines(&out);
    assert!(
        rows.iter()
            .any(|row| row["status"] == "released_after_exception")
    );
    assert_eq!(rows.last().unwrap()["reason"], "empty");
    assert_eq!(rows.last().unwrap()["jobs_processed"], 2);
    assert_eq!(queue.size(None).await.unwrap(), 0);
}

async fn verify_job_rate_limits(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
    redis: bool,
) {
    let count = |name: &'static str| async move {
        db.table("receipts")
            .r#where("name", "=", name)
            .count("*")
            .await
            .unwrap()
    };
    for kind in [
        "rate_alpha",
        "rate_alpha",
        "rate_beta",
        "rate_drop",
        "rate_unlimited",
        "rate_missing",
    ] {
        request(address, kind).await;
    }
    let args = ["queue:work", "--json", "--stop-when-empty", "--tries=3"];
    let output = run(root, &args).await;
    let rows = json_lines(&output);
    assert!(
        rows.iter()
            .any(|row| row["job"] == "rate_alpha" && row["result"] == "released")
    );
    assert_eq!(count("rate_alpha").await, 1);
    assert_eq!(count("rate_beta").await, 1);
    assert_eq!(count("rate_drop").await, 0);
    assert_eq!(count("rate_unlimited").await, 1);
    assert_eq!(count("rate_missing").await, 1);
    assert_eq!(queue.size(None).await.unwrap(), 1);
    sleep(Duration::from_secs(2)).await;
    run(root, &args).await;
    assert_eq!(count("rate_alpha").await, 2);
    assert_eq!(queue.size(None).await.unwrap(), 0);

    if redis {
        request(address, "rate_redis").await;
        request(address, "rate_redis").await;
        let args = ["queue:work", "--json", "--once", "--tries=2"];
        tokio::join!(run(root, &args), run(root, &args));
        assert_eq!(count("rate_redis").await, 1);
        assert_eq!(queue.size(None).await.unwrap(), 0);
    }
}

async fn verify_job_overlap(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let args = [
        "queue:work",
        "--json",
        "--queue=overlap",
        "--once",
        "--tries=2",
    ];
    let count = |name: &'static str| async move {
        db.table("receipts")
            .r#where("name", "=", name)
            .count("*")
            .await
            .unwrap()
    };
    let gate = root.join("overlap.ready");
    let start = |kind| async move {
        request(address, kind).await;
        let worker = artisan(root, &args).stdout(Stdio::piped()).spawn().unwrap();
        timeout(Duration::from_secs(5), async {
            while count(kind).await == 0 {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        worker
    };
    let payload = Payload::new("overlap_holder", ()).unwrap();
    assert_eq!(
        WithoutOverlapping::new("resource").getLockKey(&payload),
        "laravel-queue-overlap:overlap_holder:resource",
    );
    assert_eq!(
        WithoutOverlapping::new("resource")
            .shared()
            .withPrefix("fixture-overlap:")
            .getLockKey(&payload),
        "fixture-overlap:resource",
    );

    let worker = start("overlap_holder").await;
    for kind in [
        "overlap_deferred",
        "overlap_discard",
        "overlap_isolated",
        "overlap_other_prefix",
    ] {
        request(address, kind).await;
        let output = run(root, &args).await;
        let rows = json_lines(&output);
        let outcome = rows.last().unwrap();
        let expected = if kind == "overlap_deferred" {
            "released"
        } else {
            "deleted"
        };
        assert_eq!(outcome["result"], expected, "{output}");
    }
    assert_eq!(count("overlap_deferred").await, 0);
    assert_eq!(count("overlap_discard").await, 0);
    assert_eq!(count("overlap_isolated").await, 1);
    assert_eq!(count("overlap_other_prefix").await, 1);

    std::fs::write(&gate, []).unwrap();
    assert!(
        timeout(Duration::from_secs(5), worker.wait_with_output())
            .await
            .unwrap()
            .unwrap()
            .status
            .success()
    );
    std::fs::remove_file(&gate).unwrap();
    sleep(Duration::from_secs(3)).await;
    let output = run(root, &args).await;
    let rows = json_lines(&output);
    assert_eq!(rows.last().unwrap()["job"], "overlap_deferred");
    assert_eq!(rows.last().unwrap()["attempts"], 2);
    assert_eq!(count("overlap_deferred").await, 1);

    let worker = start("overlap_lease_holder").await;
    request(address, "overlap_lease_probe").await;
    run(root, &args).await;
    assert_eq!(count("overlap_lease_probe").await, 0);
    sleep(Duration::from_secs(3)).await;
    request(address, "overlap_lease_probe").await;
    run(root, &args).await;
    assert_eq!(count("overlap_lease_probe").await, 1);
    std::fs::write(&gate, []).unwrap();
    assert!(
        timeout(Duration::from_secs(5), worker.wait_with_output())
            .await
            .unwrap()
            .unwrap()
            .status
            .success()
    );

    request(address, "overlap_error").await;
    let output = run(
        root,
        &[
            "queue:work",
            "--json",
            "--queue=overlap",
            "--once",
            "--tries=1",
        ],
    )
    .await;
    assert_eq!(json_lines(&output).last().unwrap()["status"], "failed");
    request(address, "overlap_after_error").await;
    run(root, &args).await;
    assert_eq!(count("overlap_after_error").await, 1);
    assert_eq!(queue.size(Some("overlap")).await.unwrap(), 0);
}

async fn verify_conditional_middleware(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    let args = [
        "queue:work",
        "--json",
        "--queue=conditional",
        "--once",
        "--tries=2",
    ];
    let count = |name: &'static str| async move {
        db.table("receipts")
            .r#where("name", "=", name)
            .count("*")
            .await
            .unwrap()
    };
    for kind in [
        "conditional_skip_when",
        "conditional_skip_unless",
        "conditional_run_when",
        "conditional_run_unless",
    ] {
        request(address, kind).await;
        let out = run(root, &args).await;
        assert_eq!(
            json_lines(&out).last().unwrap()["result"],
            "deleted",
            "{out}"
        );
        assert_eq!(
            count(kind).await,
            u64::from(kind.starts_with("conditional_run"))
        );
        if matches!(kind, "conditional_skip_unless" | "conditional_run_when") {
            assert_eq!(
                out.matches(&format!("conditional-evaluation:{kind}"))
                    .count(),
                1
            );
        }
    }
    let gate = root.join("conditional.ready");
    for kind in ["conditional_release_zero", "conditional_release_delayed"] {
        request(address, kind).await;
        let out = run(root, &args).await;
        assert_eq!(
            json_lines(&out).last().unwrap()["result"],
            "released",
            "{out}"
        );
        assert_eq!(
            out.matches(&format!("conditional-evaluation:{kind}"))
                .count(),
            1
        );
        assert_eq!(count(kind).await, 0);
        assert_eq!(queue.size(Some("conditional")).await.unwrap(), 1);
        if kind == "conditional_release_delayed" {
            let out = run(root, &args).await;
            assert!(json_lines(&out).is_empty(), "{out}");
            sleep(Duration::from_secs(2)).await;
        }
        std::fs::write(&gate, []).unwrap();
        let out = run(root, &args).await;
        let rows = json_lines(&out);
        let completed = rows.last().unwrap();
        assert_eq!(completed["result"], "deleted", "{out}");
        assert_eq!(completed["attempts"], 2);
        assert_eq!(count(kind).await, 1);
        std::fs::remove_file(&gate).unwrap();
        assert_eq!(queue.size(Some("conditional")).await.unwrap(), 0);
    }
}

async fn verify_exception_throttle(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
    limiter: &str,
) {
    let name = |kind| format!("throttle_{limiter}_{kind}");
    let failed_before = db.table("failed_jobs").count("*").await.unwrap();
    let queue_name = format!("throttle-{limiter}");
    let queue_arg = format!("--queue={queue_name}");
    let args = [
        "queue:work",
        "--json",
        "--stop-when-empty",
        "--tries=3",
        "--backoff=60",
        &queue_arg,
    ];
    for kind in [
        "discard", "terminal", "bypass", "retry", "success", "retry", "retry", "blocked",
    ] {
        request(address, &name(kind)).await;
    }
    let output = run(root, &args).await;
    let rows = json_lines(&output);
    let result = |kind| {
        rows.iter()
            .find(|row| row["job"] == name(kind) && row["status"] != "starting")
            .unwrap()
    };
    assert_eq!(result("discard")["result"], "deleted", "{output}");
    assert_eq!(result("terminal")["status"], "failed", "{output}");
    assert_eq!(result("bypass")["result"], "released", "{output}");
    assert_eq!(result("blocked")["result"], "released", "{output}");
    assert_eq!(
        output
            .matches(&format!("throttle-report:{}", name("retry")))
            .count(),
        3
    );
    if limiter == "redis" {
        assert_eq!(output.matches("throttle-redis:2").count(), 4, "{output}");
        assert_eq!(output.matches("throttle-redis:1").count(), 1, "{output}");
    }
    let count = |kind| {
        let job = name(kind);
        async move {
            db.table("receipts")
                .r#where("name", "=", job)
                .count("*")
                .await
                .unwrap()
        }
    };
    assert_eq!(count("retry").await, 3);
    assert_eq!(count("success").await, 1);
    assert_eq!(count("blocked").await, 0);
    assert_eq!(
        db.table("failed_jobs").count("*").await.unwrap(),
        failed_before + 1
    );
    assert_eq!(queue.size(Some(&queue_name)).await.unwrap(), 5);
    sleep(Duration::from_secs(if limiter == "redis" { 9 } else { 12 })).await;
    run(root, &args).await;
    assert_eq!(count("blocked").await, 1);
    // The three middleware backoffs and the ordinary bypass backoff are still pending.
    assert_eq!(count("retry").await, 3);
    assert_eq!(count("bypass").await, 1);
    assert_eq!(queue.size(Some(&queue_name)).await.unwrap(), 5);
}

async fn verify_custom_disconnect(
    root: &Path,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    request(address, "flaky").await;
    let args = ["queue:work", "--json", "--stop-when-empty", "--tries=2"];
    let output = timeout(
        Duration::from_secs(8),
        artisan(root, &args)
            .env("QUEUE_WORKER_TEST_CUSTOM_DETECTOR", "1")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        json_lines(&output).last().unwrap()["reason"],
        "lost_connection"
    );
    assert_eq!(queue.size(None).await.unwrap(), 1);
    let output = run(root, &args).await;
    assert_eq!(json_lines(&output).last().unwrap()["reason"], "empty");
    assert_eq!(queue.size(None).await.unwrap(), 0);
}

async fn verify_mysql_disconnect(
    root: &Path,
    db: &openrust::database::Connection,
    queue: &dyn openrust::contracts::queue::Queue,
    address: SocketAddr,
) {
    for keep_running in [false, true] {
        request(address, "mysql_disconnect").await;
        request(address, "control_default").await;
        let mut command = artisan(
            root,
            &["queue:work", "--json", "--stop-when-empty", "--tries=2"],
        );
        if keep_running {
            command.env("QUEUE_WORKER_TEST_CONTINUE_ON_DISCONNECT", "1");
        }
        let mut worker = command.stdout(Stdio::piped()).spawn().unwrap();
        let mut stdout = BufReader::new(worker.stdout.take().unwrap());
        let id = timeout(Duration::from_secs(8), async {
            loop {
                let mut line = String::new();
                assert!(
                    stdout.read_line(&mut line).await.unwrap() > 0,
                    "worker stopped before the SQL fault"
                );
                if let Some(id) = line.trim().strip_prefix("lost-mysql:") {
                    break id.parse::<u64>().unwrap();
                }
            }
        })
        .await
        .unwrap();
        db.unprepared(&format!("KILL CONNECTION {id}"))
            .await
            .unwrap();
        let mut output = String::new();
        timeout(Duration::from_secs(10), stdout.read_to_string(&mut output))
            .await
            .unwrap()
            .unwrap();
        assert!(worker.wait().await.unwrap().success(), "{output}");
        assert!(output.contains("lost-report:mysql"), "{output}");
        let rows = json_lines(&output);
        let stop = rows.last().unwrap();
        assert_eq!(stop["exit_code"], 0);
        if keep_running {
            assert_eq!(stop["reason"], "empty");
            assert_eq!(stop["jobs_processed"], 3);
        } else {
            assert_eq!(stop["reason"], "lost_connection");
            assert_eq!(stop["jobs_processed"], 1);
            assert_eq!(queue.size(None).await.unwrap(), 2);
            run(root, &["queue:work", "--stop-when-empty", "--tries=2"]).await;
        }
        assert_eq!(queue.size(None).await.unwrap(), 0);
    }
}

async fn verify_redis_disconnect(root: &Path, port: u16, address: SocketAddr) {
    for keep_running in [false, true] {
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        let dropped = Arc::new(AtomicBool::new(false));
        let proxy_fault = dropped.clone();
        let relay = tokio::spawn(async move {
            let mut sockets = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    result = proxy.accept() => {
                        let (mut client, _) = result.unwrap();
                        let fault = proxy_fault.clone();
                        sockets.spawn(async move {
                            let Ok(mut backend) = TcpStream::connect(("127.0.0.1", port)).await else { return };
                            let (mut input, mut output) = client.split();
                            let (mut read, mut write) = backend.split();
                            let forward = async {
                                let mut buf = [0; 4096];
                                let mut matched = 0;
                                while let Ok(n @ 1..) = input.read(&mut buf).await {
                                    for &byte in &buf[..n] {
                                        matched = if byte == b"BLPOP"[matched] {
                                            matched + 1
                                        } else {
                                            usize::from(byte == b'B')
                                        };
                                        if matched == 5 {
                                            matched = 0;
                                            if !fault.swap(true, Ordering::SeqCst) {
                                                return;
                                            }
                                        }
                                    }
                                    if write.write_all(&buf[..n]).await.is_err() {
                                        return;
                                    }
                                }
                            };
                            tokio::select! {
                                () = forward => {},
                                _ = tokio::io::copy(&mut read, &mut output) => {},
                            }
                        });
                    }
                    _ = sockets.join_next(), if !sockets.is_empty() => {},
                }
            }
        });
        let mut command = artisan(
            root,
            &["queue:work", "--json", "--max-jobs=1", "--max-time=8"],
        );
        command
            .env("QUEUE_WORKER_DISABLE_POLLING", "1")
            .env("QUEUE_WORKER_TEST_REDIS_PROXY", proxy_port.to_string());
        if keep_running {
            command.env("QUEUE_WORKER_TEST_CONTINUE_ON_DISCONNECT", "1");
        }
        let worker = command.stdout(Stdio::piped()).spawn().unwrap();
        timeout(Duration::from_secs(5), async {
            while !dropped.load(Ordering::SeqCst) {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        if keep_running {
            request(address, "control_default").await;
        }
        let output = timeout(Duration::from_secs(10), worker.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        relay.abort();
        assert!(output.status.success(), "{output:?}");
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains("lost-report:redis"), "{output}");
        let rows = json_lines(&output);
        let stop = rows.last().unwrap();
        assert_eq!(stop["exit_code"], 0);
        if keep_running {
            assert_eq!(stop["reason"], "max_jobs");
            assert_eq!(stop["jobs_processed"], 1);
        } else {
            assert_eq!(stop["reason"], "lost_connection");
            assert_eq!(stop["jobs_processed"], 0);
        }
    }
}

#[tokio::test]
async fn artisan_worker_database_retries_failures_and_hydrates_context() {
    exercise(None, None).await;
}

#[tokio::test]
async fn artisan_worker_redis_retries_failures_and_hydrates_context() {
    let port = std::env::var("OPENRUST_TEST_REDIS_PORT").ok();
    assert!(port.is_some() || std::env::var_os("OPENRUST_REQUIRE_QUEUE_REDIS").is_none());
    if let Some(port) = port {
        exercise(Some(port.parse().unwrap()), None).await;
    }
}

#[tokio::test]
async fn artisan_worker_mysql_retries_failures_and_hydrates_context() {
    let url = std::env::var("OPENRUST_TEST_WORKER_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_WORKER_MYSQL").is_none());
    if let Some(url) = url {
        exercise(None, Some(&url)).await;
    }
}
