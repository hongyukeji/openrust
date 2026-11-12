//! Runs real Artisan workers and their isolated job executors.
#[cfg(not(unix))]
fn main() {}

#[cfg(unix)]
mod unix {
    use std::{
        error::Error,
        path::{Path, PathBuf},
        process::ExitCode,
        sync::Arc,
        time::Duration,
    };

    use openrust::{
        contracts::events::{Event as EventPayload, Listener},
        contracts::{container::Container as _, foundation::Application as _, queue::JobHandler},
        database::DatabaseManager,
        events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
        foundation::{Application, runApplication},
        http::HttpFuture,
        queue::events::JobTimedOut,
        queue::{
            DispatchOptions, JobContext, JobOptions, Payload, QueueError, QueueFuture,
            QueueManager, QueueResult,
        },
        support::{
            ProviderResult, ServiceProvider,
            facades::{Event, Process, Queue},
        },
    };
    use serde_json::{Value, json};

    type SetupError = Box<dyn Error + Send + Sync>;

    struct Probe(PathBuf);
    impl JobHandler for Probe {
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
                    "cpu" => loop {
                        std::hint::spin_loop();
                    },
                    "blocking" | "continue" => {
                        std::thread::sleep(Duration::from_secs(2));
                        std::fs::write(self.0.join(format!("{kind}.late")), []).map_err(error)?;
                    }
                    "external" => {
                        Process::run(format!(
                            "cd '{}' && printf ready > external.started && sleep 2 && touch external.late",
                            self.0.display()
                        )).await.map_err(|source| QueueError::Execution { source: Box::new(source) })?;
                    }
                    _ => unreachable!(),
                }
                Ok(())
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
                assert_eq!(event.timeout, Duration::from_millis(350));
                assert_executor_reaped(&self.0, kind);
                std::fs::write(self.0.join(format!("{kind}.timed_out")), []).unwrap();
                Ok(None)
            })
        }
    }

    fn assert_executor_reaped(root: &Path, kind: &str) {
        let pid = std::fs::read_to_string(root.join(format!("{kind}.pid"))).unwrap();
        assert!(
            !std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }

    struct Provider(PathBuf);
    impl ServiceProvider for Provider {
        fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                Queue::register("hard-timeout-probe", Arc::new(Probe(self.0.clone())))?;
                Event::listen(
                    JobTimedOut::NAME,
                    Arc::new(TimeoutObserver(self.0.clone())),
                    Delivery::Sync,
                )?;
                Ok(())
            })
        }
    }

    fn application(root: PathBuf) -> Result<Application, SetupError> {
        Ok(Application::configure(root.clone())
            .withConfig(|base, _| {
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
                    "database": {"default": database, "connections": connections,
                        "redis": {"options": {"prefix": format!("{name}:")}, "default": {"port": redis_port}}
                    },
                    "cache": {"default": "array", "stores": {"array": {"driver": "array"}}},
                    "queue": {"default": queue, "timed_out_exit_code": 124,
                        "kill_on_timeout": std::env::var_os("QUEUE_HARD_TEST_KEEP_RUNNING").is_none(),
                        "connections": {
                            "database": {"driver": "database", "retry_after": 2},
                            "redis": {"driver": "redis", "retry_after": 2}
                        },
                        "failed": {"driver": "database-uuids", "database": database, "table": "failed_jobs"}
                    }
                }) else { unreachable!() };
                Ok(config.into_iter().collect())
            })
            .withProviders(vec![Box::new(Provider(root))])
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
                    timeout: Some(Duration::from_millis(350)),
                    fail_on_timeout,
                    hard_timeout: true,
                    ..Default::default()
                });
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
            assert!(root.0.join(format!("{kind}.timed_out")).is_file());
            if fail_on_timeout {
                assert_eq!(queue.size(None).await.unwrap(), 0);
            } else {
                assert_eq!(queue.size(None).await.unwrap(), 1);
                let (status, lines) = artisan(&root.0, &["--once"], false).await;
                assert_eq!(status, Some(0));
                assert!(lines.is_empty(), "{lines:?}");
                tokio::time::sleep(Duration::from_secs(2)).await;
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
                timeout: Some(Duration::from_millis(350)),
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

        assert!(root.0.join("external.started").is_file());
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(!root.0.join("blocking.late").exists());
        assert!(!root.0.join("external.late").exists());
        assert!(!root.0.join("continue.late").exists());
        assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 2);
        for table in ["jobs", "failed_jobs"] {
            schema.dropIfExists(table).await.unwrap();
        }
        println!("queue hard timeout process acceptance passed on {backend}");
    }

    #[tokio::main(flavor = "current_thread")]
    pub async fn run() -> ExitCode {
        if std::env::args_os()
            .nth(1)
            .is_some_and(|argument| argument == "artisan")
        {
            return runApplication(|| {
                application(PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap()))
            })
            .await;
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

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    unix::run()
}
