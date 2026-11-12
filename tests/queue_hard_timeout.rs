//! Runs real Artisan workers and their isolated job executors.
#[cfg(not(any(unix, windows)))]
fn main() {}

#[cfg(any(unix, windows))]
mod acceptance {
    use std::{
        error::Error,
        path::{Path, PathBuf},
        process::ExitCode,
        sync::Arc,
        time::{Duration, SystemTime},
    };

    use openrust::contracts::http::Kernel as _;
    use openrust::{
        contracts::events::{Event as EventPayload, Listener},
        contracts::{container::Container as _, foundation::Application as _, queue::JobHandler},
        database::{Connection as DatabaseConnection, DatabaseManager, Value as DatabaseValue},
        events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
        foundation::{
            Application,
            configuration::{ReportDecision, Routing},
            http::Kernel,
            runApplication,
        },
        http::{HttpError, HttpFuture, Request, Response},
        log::context::Context,
        queue::{
            Delay, DispatchOptions, JobContext, JobOptions, Payload, QueueError, QueueFuture,
            QueueManager, QueueResult,
            console::ClearCommand,
            failed::{FailedJob, FailedJobProviderInterface},
        },
        queue::{
            events::{
                JobAttempted, JobExceptionOccurred, JobFailed, JobProcessed, JobProcessing,
                JobRetryRequested, JobTimedOut, QueueBusy,
            },
            middleware::{JobMiddleware, ThrottlesExceptions},
        },
        redis::{RedisManager, connections::Connection},
        routing::{Router, RoutingError},
        support::{
            ProviderResult, ServiceProvider,
            facades::{Event, Process, Queue},
        },
    };
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type SetupError = Box<dyn Error + Send + Sync>;

    #[cfg(unix)]
    const TIMEOUT: Duration = Duration::from_millis(350);
    #[cfg(windows)]
    const TIMEOUT: Duration = Duration::from_secs(2);
    const RETRY_AFTER: u64 = if cfg!(windows) { 6 } else { 2 };
    const BLOCKING_TIME: Duration = Duration::from_secs(if cfg!(windows) { 7 } else { 2 });

    #[derive(Debug)]
    struct HardPolicyError;

    impl std::fmt::Display for HardPolicyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("typed isolated retry denied")
        }
    }

    impl std::error::Error for HardPolicyError {}

    struct Probe(PathBuf, bool);
    impl JobHandler for Probe {
        fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
            if !self.1 {
                return Vec::new();
            }
            vec![Arc::new(
                ThrottlesExceptions::default()
                    .when(|error, _| {
                        std::error::Error::source(error)
                            .is_some_and(|source| source.to_string() == "hard throttle terminal")
                    })
                    .failWhen(|_| true),
            )]
        }

        fn handle<'a>(
            &'a self,
            context: &'a mut JobContext<'_>,
            payload: &'a Payload,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async move {
                let kind = payload.data().as_str().unwrap();
                let error = |source: std::io::Error| QueueError::Execution {
                    source: Box::new(source),
                };
                if context.attempts() > 1 {
                    std::fs::write(self.0.join("retried"), []).map_err(error)?;
                    return Ok(());
                }
                std::fs::write(
                    self.0.join(format!("{kind}.pid")),
                    std::process::id().to_string(),
                )
                .map_err(error)?;
                match kind {
                    "policy-terminal" => {
                        return Err(QueueError::Execution {
                            source: Box::new(HardPolicyError),
                        });
                    }
                    "throttle-terminal" => {
                        return Err(error(std::io::Error::other("hard throttle terminal")));
                    }
                    "callback-terminal"
                    | "callback-retry"
                    | "callback-error"
                    | "callback-sync"
                    | "callback-sync-error"
                    | "sync-fail"
                    | "sync-callback-error" => {
                        if kind == "callback-terminal" {
                            Context::add("callback_attempt_only", json!(true)).unwrap();
                        }
                        return Err(error(std::io::Error::other("original probe error")));
                    }
                    "deadline-refresh" | "deadline-clear" | "sync-success" | "sync-after-error" => {
                    }
                    "listener-first" | "listener-second" | "selection-default"
                    | "selection-priority" | "maintenance-probe" | "callback-follow-up" => {}
                    "listener-timeout" => {
                        Process::run(format!(
                            r#"cd '{}' && printf '%s' "$$" > listener-descendant.pid && exec sleep 4"#,
                            self.0.display()
                        ))
                        .await
                        .map_err(|source| QueueError::Execution {
                            source: Box::new(source),
                        })?;
                    }
                    "cpu" => loop {
                        std::hint::spin_loop();
                    },
                    "blocking" | "continue" => {
                        std::thread::sleep(BLOCKING_TIME);
                        std::fs::write(self.0.join(format!("{kind}.late")), []).map_err(error)?;
                    }
                    "external" => {
                        #[cfg(windows)]
                        Process::run([
                            std::env::current_exe().map_err(error)?.into_os_string(),
                            "external-fixture".into(),
                            self.0.as_os_str().into(),
                        ])
                        .await
                        .map_err(|source| QueueError::Execution {
                            source: Box::new(source),
                        })?;
                        #[cfg(unix)]
                        Process::run(format!(
                            r#"cd '{}' && printf '%s' "$$" > external-shell.pid; sleep 2 & child=$!; printf '%s' "$child" > external-sleep.pid; printf ready > external.started; wait "$child" && touch external.late"#,
                            self.0.display()
                        )).await.map_err(|source| QueueError::Execution { source: Box::new(source) })?;
                    }
                    _ => unreachable!(),
                }
                Ok(())
            })
        }

        #[allow(non_snake_case, reason = "Implements Laravel's retryUntil job method")]
        fn retryUntil(&self, payload: &Payload) -> QueueResult<Option<SystemTime>> {
            match payload.data().as_str() {
                Some("deadline-refresh") => Ok(Some(SystemTime::now() + Duration::from_secs(60))),
                Some("deadline-clear") => Ok(None),
                Some("deadline-error") => Err(QueueError::Execution {
                    source: Box::new(std::io::Error::other("retry deadline unavailable")),
                }),
                _ => Ok(payload.options().retry_until),
            }
        }

        fn failed<'a>(
            &'a self,
            context: &'a mut JobContext<'_>,
            payload: &'a Payload,
            cause: &'a QueueError,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async move {
                use std::io::Write;
                let kind = payload.data().as_str().unwrap();
                let error = |source: std::io::Error| QueueError::Execution {
                    source: Box::new(source),
                };
                let mut marker = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(self.0.join(format!("{kind}.failed")))
                    .map_err(error)?;
                writeln!(
                    marker,
                    "{}:{}:{}:{cause}",
                    context.connection(),
                    context.queue(),
                    context.attempts()
                )
                .map_err(error)?;
                if kind == "callback-terminal" {
                    assert_eq!(Context::get("callback_trace").unwrap(), Some(json!(kind)));
                    assert_eq!(Context::get("callback_attempt_only").unwrap(), None);
                }
                if matches!(
                    kind,
                    "callback-error" | "callback-sync-error" | "sync-callback-error"
                ) {
                    return Err(error(std::io::Error::other("failure callback error")));
                }
                Ok(())
            })
        }
    }

    struct FailedObserver(PathBuf);

    impl Listener for FailedObserver {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event
                    .downcast_ref::<JobFailed>()
                    .ok_or(EventError::InvalidPayload)?;
                let kind = event.job.payload.data().as_str().unwrap();
                if matches!(kind, "callback-terminal" | "callback-error") {
                    assert!(self.0.join(format!("{kind}.failed")).is_file());
                    std::fs::write(self.0.join(format!("{kind}.event")), &event.exception).unwrap();
                }
                Ok(None)
            })
        }
    }

    struct TimeoutObserver(PathBuf);

    impl Listener for TimeoutObserver {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event
                    .downcast_ref::<JobTimedOut>()
                    .ok_or(EventError::InvalidPayload)?;
                let kind = event.job.payload.data().as_str().unwrap();
                assert_eq!(event.timeout, TIMEOUT);
                assert_executor_reaped(&self.0, kind);
                #[cfg(windows)]
                if kind == "external" {
                    for member in ["external-shell.pid", "external-sleep.pid"] {
                        let pid = std::fs::read_to_string(self.0.join(member)).unwrap();
                        assert!(
                            !process_exists(&pid),
                            "Job Object member {pid} is still present"
                        );
                    }
                }
                std::fs::write(self.0.join(format!("{kind}.timed_out")), []).unwrap();
                Ok(None)
            })
        }
    }

    #[cfg(unix)]
    fn process_exists(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    #[cfg(windows)]
    fn process_exists(pid: &str) -> bool {
        let result = std::process::Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("run tasklist");
        assert!(result.status.success());
        String::from_utf8_lossy(&result.stdout).contains(&format!("\"{pid}\""))
    }

    #[cfg(windows)]
    fn external_fixture(mode: &str, root: &Path) -> ExitCode {
        if mode == "external-fixture" {
            std::fs::write(
                root.join("external-shell.pid"),
                std::process::id().to_string(),
            )
            .unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("external-descendant-fixture")
                .arg(root)
                .status()
                .unwrap();
            assert!(status.success());
        } else {
            std::fs::write(
                root.join("external-sleep.pid"),
                std::process::id().to_string(),
            )
            .unwrap();
            std::fs::write(root.join("external.started"), []).unwrap();
            std::thread::sleep(BLOCKING_TIME);
            std::fs::write(root.join("external.late"), []).unwrap();
        }
        ExitCode::SUCCESS
    }

    fn assert_executor_reaped(root: &Path, kind: &str) {
        let pid = std::fs::read_to_string(root.join(format!("{kind}.pid"))).unwrap();
        assert!(
            !process_exists(&pid),
            "{kind} executor {pid} is still present"
        );
    }

    async fn assert_external_processes_reaped(root: &Path) {
        let shell = std::fs::read_to_string(root.join("external-shell.pid")).unwrap();
        let sleep = std::fs::read_to_string(root.join("external-sleep.pid")).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while process_exists(&shell) || process_exists(&sleep) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("external shell and sleep must be reaped before their natural completion");
    }

    struct RetryObserver(PathBuf);

    impl Listener for RetryObserver {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event
                    .downcast_ref::<JobRetryRequested>()
                    .ok_or(EventError::InvalidPayload)?;
                let job = &event.job;
                let queue = Queue::connection(Some(&job.connection)).unwrap();
                let pending = queue.size(Some(&job.queue)).await.unwrap();
                std::fs::write(
                    self.0.join(format!("retry-{}.json", job.id)),
                    serde_json::to_vec(&json!({"pending":pending, "job":job})).unwrap(),
                )
                .unwrap();
                Ok(None)
            })
        }
    }

    struct BusyObserver(PathBuf);

    impl Listener for BusyObserver {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event
                    .downcast_ref::<QueueBusy>()
                    .ok_or(EventError::InvalidPayload)?;
                std::fs::write(self.0.join("busy.json"), serde_json::to_vec(event).unwrap())
                    .unwrap();
                Ok(None)
            })
        }
    }

    struct SyncObserver(PathBuf);

    impl Listener for SyncObserver {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                use std::io::Write;
                macro_rules! record {
                    ($ty:ty, $value:ident, $duration:expr, $exception:expr) => {
                        if let Some($value) = event.downcast_ref::<$ty>() {
                            let job = &$value.job;
                            let kind = job.payload.data().as_str().unwrap();
                            if $value.connection_name != "sync" || !kind.starts_with("sync-") {
                                return Ok(None);
                            }
                            assert!(job.id.is_empty());
                            assert_eq!(job.queue, "sync");
                            assert_eq!(job.attempts, 1);
                            if event.name() == JobFailed::NAME {
                                assert!(self.0.join(format!("{kind}.failed")).is_file());
                            }
                            let mut journal = std::fs::OpenOptions::new().create(true).append(true)
                                .open(self.0.join(format!("{kind}.events"))).unwrap();
                            writeln!(journal, "{}", json!({
                                "name": event.name(), "duration": $duration, "exception": $exception
                            })).unwrap();
                            if matches!((kind, event.name()), ("sync-before-error", JobProcessing::NAME) | ("sync-after-error", JobProcessed::NAME)) {
                                return Err(EventError::Listener { source: Box::new(std::io::Error::other("sync observer failure")) });
                            }
                            return Ok(None);
                        }
                    };
                }
                record!(JobProcessing, e, Value::Null, Value::Null);
                record!(JobProcessed, e, e.duration, Value::Null);
                record!(JobExceptionOccurred, e, Value::Null, e.exception);
                record!(JobFailed, e, Value::Null, e.exception);
                record!(JobAttempted, e, e.duration, e.exception);
                Err(EventError::InvalidPayload)
            })
        }
    }

    struct Provider(PathBuf);
    impl ServiceProvider for Provider {
        fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                if std::env::var_os("QUEUE_CLEAR_TEST_PROHIBITED").is_some() {
                    ClearCommand::prohibit(true)?;
                }
                let sync_observer = Arc::new(SyncObserver(self.0.clone()));
                for name in [
                    JobProcessing::NAME,
                    JobProcessed::NAME,
                    JobExceptionOccurred::NAME,
                    JobFailed::NAME,
                    JobAttempted::NAME,
                ] {
                    Event::listen(name, sync_observer.clone(), Delivery::Sync)?;
                }
                Event::listen(
                    JobRetryRequested::NAME,
                    Arc::new(RetryObserver(self.0.clone())),
                    Delivery::Sync,
                )?;
                Event::listen(
                    QueueBusy::NAME,
                    Arc::new(BusyObserver(self.0.clone())),
                    Delivery::Sync,
                )?;
                Queue::popUsing(
                    "probe",
                    Some(Arc::new(|pop, queues| {
                        Box::pin(async move {
                            assert_eq!(queues, "listener-probe");
                            pop.pop(Some("listener-selected")).await
                        })
                    })),
                )?;
                Queue::popUsing(
                    "priority-worker",
                    Some(Arc::new(|pop, queues| {
                        Box::pin(async move {
                            assert_eq!(queues, "selection-default,selection-fallback");
                            pop.pop_now(Some("selection-priority")).await
                        })
                    })),
                )?;
                if std::env::var_os("QUEUE_REMOVE_POP_CALLBACK").is_some() {
                    Queue::popUsing("priority-worker", None)?;
                }
                Queue::register("hard-timeout-probe", Arc::new(Probe(self.0.clone(), false)))?;
                Queue::register("hard-throttle-probe", Arc::new(Probe(self.0.clone(), true)))?;
                Event::listen(
                    JobFailed::NAME,
                    Arc::new(FailedObserver(self.0.clone())),
                    Delivery::Sync,
                )?;
                Event::listen(
                    JobTimedOut::NAME,
                    Arc::new(TimeoutObserver(self.0.clone())),
                    Delivery::Sync,
                )?;
                Ok(())
            })
        }
    }

    fn dispatch_probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let kind = request.route("kind").unwrap();
            Context::add("callback_trace", json!(kind)).unwrap();
            let payload = Payload::new("hard-timeout-probe", kind)?.with_options(JobOptions {
                max_tries: Some(1),
                ..JobOptions::default()
            });
            Queue::dispatch(
                payload,
                DispatchOptions {
                    connection: kind.starts_with("sync-").then(|| "sync".into()),
                    queue: Some("callback-probe".into()),
                    ..DispatchOptions::default()
                },
                None,
            )
            .await?;
            Ok(Response::empty(axum::http::StatusCode::NO_CONTENT))
        })
    }

    fn routes(_: &Application) -> Result<Router, RoutingError> {
        let mut router = Router::new();
        router.get("/callback/{kind}", dispatch_probe)?;
        Ok(router)
    }

    fn application(root: PathBuf) -> Result<Application, SetupError> {
        let report_root = root.clone();
        Ok(Application::configure(root.clone())
            .withConfig(|base, env| {
                let name = base.file_name().unwrap().to_str().unwrap();
                let backend = name.rsplit('-').next().unwrap();
                let database = if backend == "mysql" { "mysql" } else { "sqlite" };
                let queue = if backend == "redis" { "redis" } else { "database" };
                let mut connections = json!({"sqlite": {"driver": "sqlite", "database": base.join("queue.sqlite")}});
                if backend == "mysql" {
                    connections["mysql"] = json!({"url": std::env::var("OPENRUST_TEST_WORKER_MYSQL_URL").unwrap()});
                }
                let redis_port = std::env::var("OPENRUST_TEST_REDIS_PORT")
                    .ok().and_then(|port| port.parse::<u16>().ok()).unwrap_or(6379);
                let Value::Object(config) = json!({
                    "app": {"env": env.get_utf8("APP_ENV")?.unwrap_or("testing"),
                        "maintenance": {"driver": if backend == "redis" { "cache" } else { "file" }, "store": "redis"}
                    },
                    "database": {"default": database, "connections": connections,
                        "redis": {"options": {"prefix": format!("{name}:")}, "default": {"port": redis_port}}
                    },
                    "cache": {"default": "array", "stores": {
                        "array": {"driver": "array"}, "redis": {"driver": "redis", "connection": "default"}
                    }},
                    "queue": {"default": queue, "timed_out_exit_code": 124,
                        "kill_on_timeout": std::env::var_os("QUEUE_HARD_TEST_KEEP_RUNNING").is_none(),
                        "connections": {
                            "database": {"driver": "database", "retry_after": RETRY_AFTER},
                            "redis": {"driver": "redis", "retry_after": RETRY_AFTER},
                            "sync": {"driver": "sync"}
                        },
                        "failed": {
                            "driver": if std::env::var_os("QUEUE_PRUNE_TEST_NULL").is_some() { "null" } else { "database-uuids" },
                            "database": database,
                            "table": "failed_jobs"
                        }
                    }
                }) else { unreachable!() };
                Ok(config.into_iter().collect())
            })
            .withProviders(vec![Box::new(Provider(root))])
            .withRouting(Routing::new(routes))
            .withExceptions(move |exceptions| {
                exceptions.dontRetry::<HardPolicyError>();
                exceptions.report(move |error: &std::io::Error| {
                    if error.to_string() == "failure callback error" {
                        std::fs::write(report_root.join("callback-error.reported"), []).unwrap();
                        ReportDecision::Stop
                    } else {
                        ReportDecision::Continue
                    }
                });
            })
            .create()?)
    }

    struct TempRoot(PathBuf);
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn artisan(
        root: &Path,
        options: &[&str],
        keep_running: bool,
    ) -> (Option<i32>, Vec<Value>) {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "artisan",
                "queue:work",
                "--json",
                "--timeout=5",
                "--tries=2",
            ])
            .args(options)
            .env("APP_BASE_PATH", root)
            .kill_on_drop(true);
        if keep_running {
            command.env("QUEUE_HARD_TEST_KEEP_RUNNING", "1");
        }
        let result = tokio::time::timeout(Duration::from_secs(8), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.status.code().is_some_and(|code| code >= 0),
            "Worker exited abnormally with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        let lines: Vec<Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            !lines.is_empty() || result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        (result.status.code(), lines)
    }

    async fn verify_monitor(
        root: &Path,
        queue: &dyn openrust::contracts::queue::Queue,
        redis: Option<&Connection>,
    ) {
        let monitor = |args: &[&str]| {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["artisan", "queue:monitor"])
                .args(args)
                .env("APP_BASE_PATH", root)
                .kill_on_drop(true);
            async move {
                let output = tokio::time::timeout(Duration::from_secs(8), command.output())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(output.status.success(), "{output:?}");
                String::from_utf8(output.stdout).unwrap()
            }
        };
        let busy = root.join("busy.json");
        let target = format!("{}:clear-one,clear-two", queue.get_connection_name());
        let output = monitor(&[&target, "--max=4", "--json"]).await;
        let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(rows[0]["connection"], queue.get_connection_name());
        assert_eq!(rows[0]["queue"], "clear-one");
        assert_eq!(rows[0]["size"], 3);
        assert_eq!(rows[0]["pending"], 1);
        assert_eq!(rows[0]["delayed"], 1);
        assert_eq!(rows[0]["reserved"], 1);
        assert!(rows[0]["oldest_pending"].as_u64().unwrap() > 0);
        assert_eq!(rows[0]["status"], "OK");
        assert_eq!(rows[1]["size"], 1);
        assert!(!busy.exists());

        let output = monitor(&[&target, "--max=3", "--json"]).await;
        let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(rows[0]["status"], "ALERT");
        assert_eq!(rows[1]["status"], "OK");
        let event: Value = serde_json::from_slice(&std::fs::read(&busy).unwrap()).unwrap();
        assert_eq!(
            event,
            json!({"connection_name": queue.get_connection_name(), "queue": "clear-one", "size": 3})
        );
        std::fs::remove_file(&busy).unwrap();

        let output = monitor(&["clear-one", "--max=4"]).await;
        assert!(output.contains("Pending jobs: 1"), "{output}");
        assert!(output.contains("Delayed jobs: 1"), "{output}");
        assert!(output.contains("Reserved jobs: 1"), "{output}");
        let output = monitor(&["monitor-empty,sync:default", "--json"]).await;
        let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(rows[0]["pending"], 0);
        assert!(rows[0]["oldest_pending"].is_null());
        assert_eq!(rows[1]["size"], 0);
        assert_eq!(rows[1]["pending"], 0);
        assert_eq!(rows[1]["delayed"], 0);
        assert_eq!(rows[1]["reserved"], 0);
        assert!(rows[1]["oldest_pending"].is_null());

        if let Some(redis) = redis {
            let key = redis.key("queues:monitor-legacy");
            let raw = json!({"id":"legacy","payload":"{}","attempts":0}).to_string();
            redis
                .command("RPUSH", &[key.as_bytes(), raw.as_bytes()])
                .await
                .unwrap();
            let output = monitor(&["monitor-legacy", "--json"]).await;
            let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
            assert_eq!(rows[0]["pending"], 1);
            assert!(rows[0]["oldest_pending"].is_null());
            queue
                .clearable()
                .unwrap()
                .clear("monitor-legacy")
                .await
                .unwrap();
        }
    }

    async fn verify_clear(
        root: &Path,
        queue: &dyn openrust::contracts::queue::Queue,
        redis: Option<&Connection>,
    ) {
        let clear = |args: &[&str], production: bool, prohibited: bool| {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["artisan", "queue:clear"])
                .args(args)
                .env("APP_BASE_PATH", root)
                .env("APP_ENV", if production { "production" } else { "testing" })
                .kill_on_drop(true);
            if prohibited {
                command.env("QUEUE_CLEAR_TEST_PROHIBITED", "1");
            }
            async move {
                let result = tokio::time::timeout(Duration::from_secs(8), command.output())
                    .await
                    .unwrap()
                    .unwrap();
                (
                    result.status.code(),
                    String::from_utf8(result.stdout).unwrap(),
                    String::from_utf8(result.stderr).unwrap(),
                )
            }
        };
        let payload = Payload::new("hard-timeout-probe", "cpu").unwrap();
        queue.push(&payload, Some("clear-one")).await.unwrap();
        let mut reserved = queue.pop(Some("clear-one")).await.unwrap().unwrap();
        queue.push(&payload, Some("clear-one")).await.unwrap();
        queue
            .later(
                Delay::For(Duration::from_secs(60)),
                &payload,
                Some("clear-one"),
            )
            .await
            .unwrap();
        queue.push(&payload, Some("clear-two")).await.unwrap();
        queue.push(&payload, Some("keep")).await.unwrap();
        assert_eq!(queue.size(Some("clear-one")).await.unwrap(), 3);
        if let Some(redis) = redis {
            let notify = redis.key("queues:clear-one:notify");
            assert_eq!(
                redis.command("EXISTS", &[notify.as_bytes()]).await.unwrap(),
                openrust::redis::Value::Int(1)
            );
        }

        verify_monitor(root, queue, redis).await;

        let selection = [
            "--queue=clear-one, , clear-two, clear-one",
            "--no-interaction",
        ];
        let (code, _, error) = clear(&selection, true, false).await;
        assert_eq!(code, Some(1), "{error}");
        assert!(error.contains("Command cancelled."), "{error}");
        assert_eq!(queue.size(Some("clear-one")).await.unwrap(), 3);
        assert_eq!(queue.size(Some("clear-two")).await.unwrap(), 1);

        let connection = queue.get_connection_name();
        let args = [connection, selection[0], "--force"];
        let (code, _, error) = clear(&args, true, true).await;
        assert_eq!(code, Some(1), "{error}");
        assert!(error.contains("Queue clearing is prohibited."), "{error}");
        assert_eq!(queue.size(Some("clear-one")).await.unwrap(), 3);

        let (code, out, error) = clear(&args, true, false).await;
        assert_eq!(code, Some(0), "{error}");
        assert!(
            out.contains("Cleared 4 jobs from the [clear-one, clear-two] queues"),
            "{out}"
        );
        assert_eq!(queue.size(Some("clear-one")).await.unwrap(), 0);
        assert_eq!(queue.size(Some("clear-two")).await.unwrap(), 0);
        if let Some(redis) = redis {
            let notify = redis.key("queues:clear-one:notify");
            let keep_notify = redis.key("queues:keep:notify");
            assert_eq!(
                redis.command("EXISTS", &[notify.as_bytes()]).await.unwrap(),
                openrust::redis::Value::Int(0)
            );
            assert_eq!(
                redis
                    .command("EXISTS", &[keep_notify.as_bytes()])
                    .await
                    .unwrap(),
                openrust::redis::Value::Int(1)
            );
        }
        assert_eq!(queue.size(Some("keep")).await.unwrap(), 1);
        assert!(matches!(
            reserved.delete().await,
            Err(QueueError::LostReservation)
        ));

        let (code, _, error) = clear(&["sync"], false, false).await;
        assert_eq!(code, Some(1), "{error}");
        assert!(
            error.contains("Clearing queues is not supported"),
            "{error}"
        );

        queue.push(&payload, None).await.unwrap();
        let (code, out, error) = clear(&[], false, false).await;
        assert_eq!(code, Some(0), "{error}");
        assert!(
            out.contains("Cleared 1 job from the [default] queue"),
            "{out}"
        );
        assert_eq!(queue.size(None).await.unwrap(), 0);
        assert_eq!(queue.size(Some("keep")).await.unwrap(), 1);
        queue.clearable().unwrap().clear("keep").await.unwrap();
    }

    async fn seed_failures(db: &DatabaseConnection, prefix: &str, count: usize, failed_at: &str) {
        for start in (0..count).step_by(200) {
            let end = (start + 200).min(count);
            let values = vec!["(?, 'database', 'default', '{}', 'fixture error', ?)"; end - start]
                .join(", ");
            let sql = format!(
                "insert into failed_jobs (uuid, connection, queue, payload, exception, failed_at) values {values}"
            );
            let bindings: Vec<DatabaseValue> = (start..end)
                .flat_map(|index| [format!("{prefix}-{index}").into(), failed_at.into()])
                .collect();
            db.statement(&sql, &bindings).await.unwrap();
        }
    }

    async fn verify_prune_failed(root: &Path, app: &Application, db: &DatabaseConnection) {
        let provider = app
            .container()
            .get::<dyn FailedJobProviderInterface>("queue.failer")
            .unwrap();
        let prune = provider.prunable().unwrap();
        let count = |name: &'static str| async move {
            db.table("failed_jobs")
                .r#where("uuid", "=", name)
                .count("*")
                .await
                .unwrap()
        };
        seed_failures(db, "prune-before", 1, "1999-12-31 23:59:59").await;
        seed_failures(db, "prune-equal", 1, "2000-01-01 00:00:00").await;
        seed_failures(db, "prune-after", 1, "2000-01-01 00:00:01").await;
        let cutoff = chrono::DateTime::from_timestamp(946_684_800, 0).unwrap();
        assert_eq!(prune.prune(cutoff).await.unwrap(), 1);
        assert_eq!(count("prune-before-0").await, 0);
        assert_eq!(count("prune-equal-0").await, 1);
        assert_eq!(count("prune-after-0").await, 1);
        db.table("failed_jobs")
            .r#where("uuid", "like", "prune-%")
            .delete()
            .await
            .unwrap();

        let run = |args: &[&str], null: bool| {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["artisan", "queue:prune-failed"])
                .args(args)
                .env("APP_BASE_PATH", root)
                .kill_on_drop(true);
            if null {
                command.env("QUEUE_PRUNE_TEST_NULL", "1");
            }
            async move {
                tokio::time::timeout(Duration::from_secs(8), command.output())
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
        let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
        let timestamp = |age| {
            (now - chrono::TimeDelta::hours(age))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        };
        seed_failures(db, "prune-old", 1001, &timestamp(25)).await;
        seed_failures(db, "prune-middle", 1, &timestamp(2)).await;
        seed_failures(db, "prune-fresh", 1, &timestamp(0)).await;

        let output = run(&[], true).await;
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("does not support pruning"));
        assert_eq!(count("prune-old-0").await, 1);
        let removed = |result: std::process::Output| {
            assert!(result.status.success(), "{result:?}");
            let out = String::from_utf8(result.stdout).unwrap();
            out.lines()
                .find_map(|line| line.split_once(" entries deleted."))
                .and_then(|(prefix, _)| prefix.split_whitespace().last())
                .and_then(|count| count.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("{out}"))
        };
        assert_eq!(removed(run(&[], false).await), 1001);
        assert_eq!(count("prune-middle-0").await, 1);
        assert_eq!(removed(run(&[], false).await), 0);
        assert_eq!(removed(run(&["--hours=1"], false).await), 1);
        assert_eq!(count("prune-fresh-0").await, 1);

        seed_failures(db, "prune-concurrent", 31, &timestamp(25)).await;
        let (first, second) = tokio::join!(run(&[], false), run(&[], false));
        assert_eq!(removed(first) + removed(second), 31);
        assert_eq!(count("prune-fresh-0").await, 1);
    }

    async fn verify_failed_handler(
        root: &Path,
        app: &Application,
        manager: &QueueManager,
        queue: &dyn openrust::contracts::queue::Queue,
    ) {
        let mut kernel = Kernel::new(application(root.to_owned()).unwrap());
        kernel.bootstrap().await.unwrap();
        let kernel = Arc::new(kernel);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = axum::Router::new().fallback(move |request: axum::extract::Request| {
            kernel.clone().handleTransport(request)
        });
        let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        let named = Some("callback-probe");
        let retry = Payload::new("hard-timeout-probe", "callback-retry").unwrap();
        queue.push(&retry, named).await.unwrap();
        let once = ["--once", "--queue=callback-probe"];
        let (status, _) = artisan(root, &once, false).await;
        assert_eq!(status, Some(0));
        assert!(!root.join("callback-retry.failed").exists());
        assert_eq!(queue.size(named).await.unwrap(), 1);
        let (status, lines) = artisan(root, &once, false).await;
        assert_eq!(status, Some(0));
        assert_eq!(lines.last().unwrap()["status"], "success");
        assert!(!root.join("callback-retry.failed").exists());

        let request = |kind: &'static str| async move {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
                let request = format!(
                    "GET /callback/{kind} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                );
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).await.unwrap();
                response
            })
            .await
            .unwrap()
        };
        let kinds = ["callback-terminal", "callback-error"];
        for kind in kinds {
            let response = request(kind).await;
            assert!(response.starts_with("HTTP/1.1 204 "), "{response}");
        }
        let next = Payload::new("hard-timeout-probe", "callback-follow-up").unwrap();
        queue.push(&next, named).await.unwrap();
        let (status, lines) = artisan(
            root,
            &["--queue=callback-probe", "--stop-when-empty"],
            false,
        )
        .await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["jobs_processed"], 3);
        let outcomes = lines
            .iter()
            .filter(|line| matches!(line["status"].as_str(), Some("failed" | "success")))
            .collect::<Vec<_>>();
        assert_eq!(outcomes.len(), 3, "{lines:?}");
        assert_eq!(outcomes[2]["uuid"], next.uuid());
        assert_eq!(outcomes[2]["status"], "success");
        let provider = app
            .container()
            .get::<dyn FailedJobProviderInterface>("queue.failer")
            .unwrap();
        for (kind, outcome) in kinds.into_iter().zip(outcomes) {
            assert_eq!(outcome["status"], "failed");
            let uuid = outcome["uuid"].as_str().unwrap();
            let record = provider.find(uuid).await.unwrap().unwrap();
            assert_eq!(record.payload.data().as_str(), Some(kind));
            assert_eq!(record.exception, "queue job execution failed");
            let marker = std::fs::read_to_string(root.join(format!("{kind}.failed"))).unwrap();
            assert!(marker.starts_with(&format!(
                "{}:callback-probe:1:",
                queue.get_connection_name()
            )));
            let event = std::fs::read_to_string(root.join(format!("{kind}.event"))).unwrap();
            assert!(event.contains("original probe error"), "{event}");
        }
        assert_eq!(queue.size(named).await.unwrap(), 0);
        assert!(root.join("callback-error.reported").is_file());
        for kind in [
            "sync-success",
            "sync-fail",
            "sync-callback-error",
            "sync-before-error",
            "sync-after-error",
        ] {
            let response = request(kind).await;
            let status = if kind == "sync-success" { 204 } else { 500 };
            assert!(
                response.starts_with(&format!("HTTP/1.1 {status} ")),
                "{response}"
            );
            let events = std::fs::read_to_string(root.join(format!("{kind}.events"))).unwrap();
            let events = events
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>();
            let names = events
                .iter()
                .map(|event| event["name"].as_str().unwrap())
                .collect::<Vec<_>>();
            if kind == "sync-success" {
                assert_eq!(names, ["JobProcessing", "JobProcessed", "JobAttempted"]);
                assert_eq!(events[1]["duration"], events[2]["duration"]);
                assert!(events[2]["exception"].is_null());
            } else {
                let mut expected = vec!["JobProcessing"];
                if kind == "sync-after-error" {
                    expected.push("JobProcessed");
                }
                expected.extend(["JobExceptionOccurred", "JobFailed", "JobAttempted"]);
                assert_eq!(names, expected);
                let error = if matches!(kind, "sync-before-error" | "sync-after-error") {
                    "sync observer failure"
                } else {
                    "original probe error"
                };
                assert!(
                    events.last().unwrap()["exception"]
                        .as_str()
                        .unwrap()
                        .contains(error)
                );
            }
            assert_eq!(
                events.last().unwrap()["duration"].is_null(),
                kind == "sync-before-error"
            );
            assert_eq!(
                root.join(format!("{kind}.pid")).exists(),
                kind != "sync-before-error"
            );
        }
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());

        let sync = manager.connection(Some("sync")).unwrap();
        let payload = Payload::new("hard-timeout-probe", "callback-sync").unwrap();
        let cause = sync.push(&payload, named).await.unwrap_err();
        assert_eq!(
            Error::source(&cause).unwrap().to_string(),
            "original probe error"
        );
        let marker = std::fs::read_to_string(root.join("callback-sync.failed")).unwrap();
        assert!(marker.starts_with("sync:callback-probe:1:"));

        let other = TempRoot(root.with_file_name(format!(
            "openrust-callback-other-{}-sqlite",
            std::process::id()
        )));
        std::fs::create_dir(&other.0).unwrap();
        let mut other_app = application(other.0.clone()).unwrap();
        other_app.boot().await.unwrap();
        let other_sync = other_app
            .container()
            .make::<QueueManager>()
            .unwrap()
            .connection(Some("sync"))
            .unwrap();
        let payload = Payload::new("hard-timeout-probe", "callback-sync-error").unwrap();
        let cause = other_sync.push(&payload, named).await.unwrap_err();
        assert_eq!(
            Error::source(&cause).unwrap().to_string(),
            "failure callback error"
        );
        assert!(other.0.join("callback-sync-error.failed").is_file());
        assert!(!root.join("callback-sync-error.failed").exists());
    }

    async fn verify_retry_selection(
        root: &Path,
        app: &Application,
        db: &DatabaseConnection,
        queue: &dyn openrust::contracts::queue::Queue,
    ) {
        db.table("failed_jobs").delete().await.unwrap();
        let provider = app
            .container()
            .get::<dyn FailedJobProviderInterface>("queue.failer")
            .unwrap();
        for (id, name) in [
            ("only", "retry-filter"),
            ("1", "retry-other"),
            ("2", "retry-other"),
            ("keep", "retry-other"),
        ] {
            let payload = Payload::new("hard-timeout-probe", id).unwrap();
            let mut stored = serde_json::to_value(&payload).unwrap();
            stored["options"]
                .as_object_mut()
                .unwrap()
                .remove("hard_timeout");
            provider
                .log(&FailedJob {
                    id: id.into(),
                    connection: queue.get_connection_name().into(),
                    queue: name.into(),
                    payload,
                    exception: "original failure".into(),
                    failed_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                })
                .await
                .unwrap();
            if id == "only" {
                db.statement(
                    "update failed_jobs set payload = ? where uuid = ?",
                    &[
                        serde_json::to_string_pretty(&stored).unwrap().into(),
                        id.into(),
                    ],
                )
                .await
                .unwrap();
            }
        }
        let retry = |args: &[&str]| {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["artisan", "queue:retry"])
                .args(args)
                .env("APP_BASE_PATH", root)
                .kill_on_drop(true);
            async move {
                let result = tokio::time::timeout(Duration::from_secs(8), command.output())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(result.status.success(), "{result:?}");
                (
                    String::from_utf8(result.stdout).unwrap(),
                    String::from_utf8(result.stderr).unwrap(),
                )
            }
        };
        let (stdout, stderr) = retry(&["--queue=retry-filter", "keep", "--range=1-2"]).await;
        assert!(stdout.contains("Queued failed job only."), "{stdout}");
        assert!(stderr.is_empty(), "{stderr}");
        assert_eq!(provider.ids(None).await.unwrap().len(), 3);
        assert_eq!(queue.size(Some("retry-filter")).await.unwrap(), 1);
        assert_eq!(queue.size(Some("retry-other")).await.unwrap(), 0);
        let event: Value =
            serde_json::from_slice(&std::fs::read(root.join("retry-only.json")).unwrap()).unwrap();
        assert_eq!(event["pending"], 0);
        assert_eq!(event["job"]["exception"], "original failure");

        let (stdout, stderr) =
            retry(&["missing", "--range=2-1", "--range=bad", "--range=2-2"]).await;
        assert!(
            stderr.contains("Unable to find failed job with ID [missing]."),
            "{stderr}"
        );
        assert_eq!(stdout.matches("Queued failed job").count(), 2, "{stdout}");
        assert!(stdout.find("Queued failed job 2.") < stdout.find("Queued failed job 1."));
        assert_eq!(queue.size(Some("retry-other")).await.unwrap(), 2);
        assert_eq!(provider.ids(None).await.unwrap(), ["keep"]);
        let (stdout, stderr) = retry(&["all", "--queue=absent"]).await;
        assert!(stdout.contains("Queued failed job keep."), "{stdout}");
        assert!(stderr.is_empty(), "{stderr}");
        assert_eq!(queue.size(Some("retry-other")).await.unwrap(), 3);
        let (stdout, stderr) = retry(&["--queue=absent"]).await;
        assert!(stdout.contains("No retryable jobs found."), "{stdout}");
        assert!(
            stderr.contains("Unable to find failed jobs for queue [absent]."),
            "{stderr}"
        );
        assert!(provider.ids(None).await.unwrap().is_empty());

        if queue.get_connection_name() == "database" {
            let payload = Payload::new("hard-timeout-probe", "race").unwrap();
            provider
                .log(&FailedJob {
                    id: payload.uuid().into(),
                    connection: "database".into(),
                    queue: "retry-race".into(),
                    payload,
                    exception: "original failure".into(),
                    failed_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                })
                .await
                .unwrap();
            let args = ["--queue=retry-race"];
            let ((first, _), (second, _)) = tokio::join!(retry(&args), retry(&args));
            assert_eq!(
                first.matches("Queued failed job").count()
                    + second.matches("Queued failed job").count(),
                1
            );
            assert_eq!(queue.size(Some("retry-race")).await.unwrap(), 1);
            assert!(provider.ids(None).await.unwrap().is_empty());
        }
    }

    async fn verify_retry_deadline(
        root: &Path,
        app: &Application,
        queue: &dyn openrust::contracts::queue::Queue,
    ) {
        let provider = app
            .container()
            .get::<dyn FailedJobProviderInterface>("queue.failer")
            .unwrap();
        let expired = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        for kind in [
            "deadline-refresh",
            "deadline-clear",
            "deadline-fixed",
            "deadline-error",
        ] {
            let payload = Payload::new("hard-timeout-probe", kind)
                .unwrap()
                .with_options(JobOptions {
                    retry_until: Some(expired),
                    ..JobOptions::default()
                });
            provider
                .log(&FailedJob {
                    id: kind.into(),
                    connection: queue.get_connection_name().into(),
                    queue: kind.into(),
                    payload,
                    exception: "original failure".into(),
                    failed_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                })
                .await
                .unwrap();
            let mut retry = tokio::process::Command::new(std::env::current_exe().unwrap());
            retry
                .args(["artisan", "queue:retry", kind])
                .env("APP_BASE_PATH", root)
                .kill_on_drop(true);
            let result = tokio::time::timeout(Duration::from_secs(8), retry.output())
                .await
                .unwrap()
                .unwrap();
            if kind == "deadline-error" {
                assert_eq!(result.status.code(), Some(1), "{result:?}");
                assert!(provider.find(kind).await.unwrap().is_some());
                assert_eq!(queue.size(Some(kind)).await.unwrap(), 0);
                continue;
            }
            assert!(result.status.success(), "{result:?}");
            assert!(provider.find(kind).await.unwrap().is_none());
            let mut queued = queue.pop(Some(kind)).await.unwrap().unwrap();
            let deadline = queued.options().retry_until;
            match kind {
                "deadline-refresh" => assert!(deadline.unwrap() > SystemTime::now()),
                "deadline-clear" => assert!(deadline.is_none()),
                "deadline-fixed" => assert_eq!(deadline, Some(expired)),
                _ => unreachable!(),
            }
            queued.release(Duration::ZERO).await.unwrap();
            let (status, lines) =
                artisan(root, &["--once", &format!("--queue={kind}")], false).await;
            assert_eq!(status, Some(0), "{lines:?}");
            let expected = if kind == "deadline-fixed" {
                "failed"
            } else {
                "success"
            };
            assert_eq!(lines.last().unwrap()["status"], expected, "{lines:?}");
        }
    }

    async fn verify_listener(root: &Path, queue: &dyn openrust::contracts::queue::Queue) {
        let command = || {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "artisan",
                    "queue:listen",
                    "--name=probe",
                    "--queue=listener-probe",
                    "--sleep=1",
                    "--rest=0",
                    "--memory=0",
                    "--env=testing",
                ])
                .env("APP_BASE_PATH", root)
                .kill_on_drop(true);
            command
        };
        for kind in ["listener-first", "listener-second"] {
            let payload = Payload::new("hard-timeout-probe", kind).unwrap();
            queue
                .push(&payload, Some("listener-selected"))
                .await
                .unwrap();
        }
        let listener = command()
            .arg("--timeout=5")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if root.join("listener-first.pid").is_file()
                    && root.join("listener-second.pid").is_file()
                    && queue.size(Some("listener-selected")).await.unwrap() == 0
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let first = std::fs::read_to_string(root.join("listener-first.pid")).unwrap();
        let second = std::fs::read_to_string(root.join("listener-second.pid")).unwrap();
        assert_ne!(first, second);
        assert!(
            std::process::Command::new("kill")
                .args(["-TERM", &listener.id().unwrap().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let output = tokio::time::timeout(Duration::from_secs(8), listener.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("Processing jobs from the [listener-probe] queue."),
            "{stdout}"
        );
        assert_eq!(stdout.matches("hard-timeout-probe").count(), 4, "{stdout}");
        assert_executor_reaped(root, "listener-first");
        assert_executor_reaped(root, "listener-second");

        let payload = Payload::new("hard-timeout-probe", "listener-timeout").unwrap();
        queue
            .push(&payload, Some("listener-selected"))
            .await
            .unwrap();
        let output = tokio::time::timeout(
            Duration::from_secs(8),
            command().arg("--timeout=1").output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("process exceeded its timeout"),
            "{output:?}"
        );
        assert_executor_reaped(root, "listener-timeout");
        assert_executor_reaped(root, "listener-descendant");
        assert_eq!(queue.size(Some("listener-selected")).await.unwrap(), 1);
    }

    async fn verify_named_pop(root: &Path, queue: &dyn openrust::contracts::queue::Queue) {
        for name in ["selection-default", "selection-priority"] {
            let payload = Payload::new("hard-timeout-probe", name).unwrap();
            queue.push(&payload, Some(name)).await.unwrap();
        }
        let args = [
            "--once",
            "--name=priority-worker",
            "--queue=selection-default,selection-fallback",
        ];
        let (status, lines) = artisan(root, &args, false).await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["queue"], "selection-priority");
        assert!(root.join("selection-priority.pid").is_file());
        assert!(!root.join("selection-default.pid").exists());
        assert_eq!(queue.size(Some("selection-default")).await.unwrap(), 1);
        let (status, lines) = artisan(root, &args, false).await;
        assert_eq!(status, Some(0));
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(queue.size(Some("selection-default")).await.unwrap(), 1);

        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["artisan", "queue:work", "--json"])
            .args(args)
            .env("APP_BASE_PATH", root)
            .env("QUEUE_REMOVE_POP_CALLBACK", "1")
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(8), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(root.join("selection-default.pid").is_file());
        assert_eq!(queue.size(Some("selection-default")).await.unwrap(), 0);

        for name in ["selection-default", "selection-priority"] {
            let payload = Payload::new("hard-timeout-probe", name).unwrap();
            queue.push(&payload, Some(name)).await.unwrap();
        }
        let mut first = application(root.to_owned()).unwrap();
        let mut second = application(root.to_owned()).unwrap();
        first.boot().await.unwrap();
        second.boot().await.unwrap();
        second
            .container()
            .make::<QueueManager>()
            .unwrap()
            .popUsing("priority-worker", None);
        let command: Vec<_> = std::iter::once("queue:work")
            .chain(args)
            .map(std::ffi::OsString::from)
            .collect();
        assert_eq!(second.handleCommand(&command).await.unwrap(), 0);
        assert_eq!(queue.size(Some("selection-default")).await.unwrap(), 0);
        assert_eq!(queue.size(Some("selection-priority")).await.unwrap(), 1);
        assert_eq!(first.handleCommand(&command).await.unwrap(), 0);
        assert_eq!(queue.size(Some("selection-priority")).await.unwrap(), 0);
    }

    async fn verify_maintenance(
        root: &Path,
        app: &Application,
        queue: &dyn openrust::contracts::queue::Queue,
    ) {
        let mode = app.maintenanceMode().unwrap();
        assert!(!app.isDownForMaintenance().await.unwrap());
        assert!(mode.data().await.unwrap().is_none());
        let state =
            serde_json::Map::from_iter([("status".into(), json!(503)), ("retry".into(), json!(5))]);
        mode.activate(&state).await.unwrap();
        assert!(app.isDownForMaintenance().await.unwrap());
        assert_eq!(mode.data().await.unwrap(), Some(state));

        let payload = Payload::new("hard-timeout-probe", "maintenance-probe").unwrap();
        let name = Some("maintenance-jobs");
        queue.push(&payload, name).await.unwrap();
        let (status, lines) = artisan(
            root,
            &["--once", "--sleep=0", "--queue=maintenance-jobs"],
            false,
        )
        .await;
        assert_eq!(status, Some(0));
        assert!(lines.is_empty(), "{lines:?}");
        let (status, lines) = artisan(
            root,
            &["--max-time=1", "--sleep=0", "--queue=maintenance-jobs"],
            false,
        )
        .await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["reason"], "max_time");
        assert_eq!(queue.size(name).await.unwrap(), 1);
        assert!(!root.join("maintenance-probe.pid").exists());

        let listen = |force| {
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "artisan",
                    "queue:listen",
                    "--queue=maintenance-jobs",
                    "--memory=1",
                    "--sleep=0",
                    "--timeout=5",
                ])
                .env("APP_BASE_PATH", root)
                .kill_on_drop(true);
            if force {
                command.arg("--force");
            }
            async move {
                let output = tokio::time::timeout(Duration::from_secs(8), command.output())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(output.status.success(), "{output:?}");
            }
        };
        listen(false).await;
        assert_eq!(queue.size(name).await.unwrap(), 1);
        listen(true).await;
        assert_eq!(queue.size(name).await.unwrap(), 0);

        queue.push(&payload, name).await.unwrap();
        let (status, lines) = artisan(
            root,
            &["--max-jobs=1", "--force", "--queue=maintenance-jobs"],
            false,
        )
        .await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["jobs_processed"], 1);
        assert_eq!(queue.size(name).await.unwrap(), 0);

        mode.deactivate().await.unwrap();
        mode.deactivate().await.unwrap();
        assert!(!app.isDownForMaintenance().await.unwrap());
        assert!(mode.data().await.unwrap().is_none());
        queue.push(&payload, name).await.unwrap();
        let (status, lines) = artisan(root, &["--once", "--queue=maintenance-jobs"], false).await;
        assert_eq!(status, Some(0));
        assert_eq!(lines.last().unwrap()["status"], "success");
        assert_eq!(queue.size(name).await.unwrap(), 0);
    }

    async fn exercise(backend: &str) {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "openrust-hard-timeout-{}-{backend}",
            std::process::id()
        )));
        std::fs::create_dir(&root.0).unwrap();
        std::fs::File::create(root.0.join("queue.sqlite")).unwrap();
        let mut app = application(root.0.clone()).unwrap();
        app.boot().await.unwrap();
        let db = app
            .container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap();
        let mut schema = db.getSchemaBuilder();
        for table in ["jobs", "failed_jobs"] {
            schema.dropIfExists(table).await.unwrap();
        }
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
        let manager = app.container().make::<QueueManager>().unwrap();
        let queue = manager.connection(None).unwrap();
        for (kind, fail_on_timeout) in [("cpu", false), ("blocking", true), ("external", true)] {
            let payload = Payload::new("hard-timeout-probe", kind)
                .unwrap()
                .with_options(JobOptions {
                    timeout: Some(TIMEOUT),
                    fail_on_timeout: fail_on_timeout && kind != "external",
                    max_exceptions: (kind == "external").then_some(1),
                    hard_timeout: true,
                    ..Default::default()
                });
            let uuid = payload.uuid().to_owned();
            manager
                .dispatch(payload, DispatchOptions::default(), None)
                .await
                .unwrap();
            let (status, lines) = artisan(&root.0, &["--stop-when-empty"], false).await;
            assert_eq!(status, Some(124), "{lines:?}");
            let stopped = lines.last().unwrap();
            assert_eq!(stopped["reason"], "timed_out");
            assert_eq!(stopped["exit_code"], 124);
            assert_executor_reaped(&root.0, kind);
            if kind == "external" {
                assert_external_processes_reaped(&root.0).await;
            }
            assert!(root.0.join(format!("{kind}.timed_out")).is_file());
            let failures = db
                .select(
                    "select exception from failed_jobs where uuid = ?",
                    &[uuid.into()],
                )
                .await
                .unwrap();
            if fail_on_timeout {
                assert_eq!(queue.size(None).await.unwrap(), 0);
                assert_eq!(failures.len(), 1);
                assert!(matches!(
                    failures[0].get("exception"),
                    Some(DatabaseValue::Text(message)) if message == "queued job timed out"
                ));
            } else {
                assert!(failures.is_empty());
                assert_eq!(queue.size(None).await.unwrap(), 1);
                let (status, lines) = artisan(&root.0, &["--once"], false).await;
                assert_eq!(status, Some(0));
                assert!(lines.is_empty(), "{lines:?}");
                tokio::time::sleep(Duration::from_secs(RETRY_AFTER)).await;
                let (status, lines) = artisan(&root.0, &["--once"], false).await;
                assert_eq!(status, Some(0));
                assert_eq!(lines.last().unwrap()["status"], "success");
                assert!(root.0.join("retried").is_file());
                assert_eq!(queue.size(None).await.unwrap(), 0);
            }
        }
        let payload = Payload::new("hard-timeout-probe", "continue")
            .unwrap()
            .with_options(JobOptions {
                timeout: Some(TIMEOUT),
                hard_timeout: true,
                ..Default::default()
            });
        std::fs::remove_file(root.0.join("retried")).unwrap();
        manager
            .dispatch(payload, DispatchOptions::default(), None)
            .await
            .unwrap();
        let (status, lines) = artisan(&root.0, &["--stop-when-empty"], true).await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["reason"], "empty");
        assert_eq!(lines.last().unwrap()["jobs_processed"], 2);
        assert!(root.0.join("continue.timed_out").is_file());
        assert!(root.0.join("retried").is_file());

        manager
            .dispatch(
                Payload::new("hard-throttle-probe", "throttle-terminal")
                    .unwrap()
                    .with_options(JobOptions {
                        hard_timeout: true,
                        ..Default::default()
                    }),
                DispatchOptions::default(),
                None,
            )
            .await
            .unwrap();
        let (status, lines) = artisan(&root.0, &["--once"], false).await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["status"], "failed");
        assert_eq!(queue.size(None).await.unwrap(), 0);

        manager
            .dispatch(
                Payload::new("hard-timeout-probe", "policy-terminal")
                    .unwrap()
                    .with_options(JobOptions {
                        hard_timeout: true,
                        ..Default::default()
                    }),
                DispatchOptions::default(),
                None,
            )
            .await
            .unwrap();
        let (status, lines) = artisan(&root.0, &["--once"], false).await;
        assert_eq!(status, Some(0), "{lines:?}");
        assert_eq!(lines.last().unwrap()["status"], "failed");
        assert_eq!(queue.size(None).await.unwrap(), 0);

        assert!(root.0.join("external.started").is_file());
        tokio::time::sleep(BLOCKING_TIME).await;
        assert!(!root.0.join("blocking.late").exists());
        assert!(!root.0.join("external.late").exists());
        assert!(!root.0.join("continue.late").exists());
        let redis = (backend == "redis").then(|| {
            app.container()
                .make::<RedisManager>()
                .unwrap()
                .connection(None)
                .unwrap()
        });
        verify_clear(&root.0, &*queue, redis.as_deref()).await;
        assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 4);
        assert!(!root.0.join("cpu.failed").exists());
        assert!(root.0.join("blocking.failed").is_file());
        assert!(root.0.join("external.failed").is_file());
        assert!(!root.0.join("continue.failed").exists());
        verify_failed_handler(&root.0, &app, &manager, &*queue).await;
        assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 6);
        if cfg!(unix) {
            if backend != "redis" {
                verify_prune_failed(&root.0, &app, &db).await;
            }
            verify_retry_selection(&root.0, &app, &db, &*queue).await;
            verify_retry_deadline(&root.0, &app, &*queue).await;
            verify_listener(&root.0, &*queue).await;
            verify_named_pop(&root.0, &*queue).await;
            verify_maintenance(&root.0, &app, &*queue).await;
        }
        for table in ["jobs", "failed_jobs"] {
            schema.dropIfExists(table).await.unwrap();
        }
        println!("queue hard timeout process acceptance passed on {backend}");
    }

    #[tokio::main(flavor = "current_thread")]
    pub async fn run() -> ExitCode {
        #[cfg(windows)]
        if let Some(mode) = std::env::args().nth(1)
            && matches!(
                mode.as_str(),
                "external-fixture" | "external-descendant-fixture"
            )
        {
            return external_fixture(&mode, Path::new(&std::env::args_os().nth(2).unwrap()));
        }
        if std::env::args_os()
            .nth(1)
            .is_some_and(|argument| argument == "artisan")
        {
            return runApplication(|| {
                application(PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap()))
            })
            .await;
        }
        #[cfg(windows)]
        {
            assert!(process_exists(&std::process::id().to_string()));
            let unassigned = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["artisan", "__queue-execute-reservation"])
                .env_remove("APP_BASE_PATH")
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert_eq!(unassigned.status.code(), Some(1));
            assert!(
                String::from_utf8_lossy(&unassigned.stderr)
                    .contains("Queue job execution process failed:")
            );
        }
        exercise("sqlite").await;
        if std::env::var_os("OPENRUST_TEST_WORKER_MYSQL_URL").is_some() {
            exercise("mysql").await;
        }
        if std::env::var_os("OPENRUST_TEST_REDIS_PORT").is_some() {
            exercise("redis").await;
        }
        ExitCode::SUCCESS
    }
}

#[cfg(any(unix, windows))]
fn main() -> std::process::ExitCode {
    acceptance::run()
}
