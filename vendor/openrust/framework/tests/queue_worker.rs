use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::Ttl,
    contracts::{
        container::Container as _,
        events::{Event as PayloadEvent, Listener},
        foundation::Application as _,
        http::Kernel as _,
        queue::JobHandler,
    },
    database::{DB, DatabaseManager, Value},
    events::{
        Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerRegistry,
        ListenerResponse, QueuedListenerOptions,
    },
    foundation::{Application, configuration::Routing, events::DiscoverEvents, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{
        DispatchOptions, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueManager,
        QueueResult, WorkerStopReason,
        events::{
            Looping, QueuePaused, QueueResumed, QueuesPaused, QueuesResumed, WorkerIdle,
            WorkerQueuePaused, WorkerQueueResumed, WorkerStarting, WorkerStopping,
        },
        middleware::{JobMiddleware, JobNext},
    },
    routing::{Router, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Context, Event, Queue},
    },
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
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
                "timeout" => {
                    sleep(Duration::from_secs(10)).await;
                    db.statement("insert into receipts (name, value) values ('late', 1)", &[])
                        .await
                        .map_err(execution)?;
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
                "pause_gate" | "restart_gate" => {
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
                assert_eq!(event.status, 0);
                assert_eq!(
                    event.last_job_processed_at.is_some(),
                    event.jobs_processed > 0
                );
                (
                    format!("stop:{:?}", event.reason),
                    i64::try_from(event.jobs_processed).unwrap(),
                )
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

struct Providers;
impl ServiceProvider for Providers {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if std::env::var_os("QUEUE_WORKER_DISABLE_POLLING").is_some() {
                Queue::withoutInterruptionPolling()?;
            }
            for name in [
                "flaky",
                "manual_release",
                "terminal",
                "timeout",
                "slow",
                "pause_gate",
                "restart_gate",
                "control_default",
                "control_low",
            ] {
                Queue::register(name, Arc::new(Handler))?;
            }
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
            let mut payload = Payload::new(kind, ())?;
            if kind == "timeout" {
                payload = payload.with_options(JobOptions {
                    timeout: Some(Duration::from_millis(100)),
                    fail_on_timeout: true,
                    ..Default::default()
                });
            }
            let options = DispatchOptions {
                queue: (kind == "control_low").then(|| "low".to_owned()),
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
            let config: serde_json::Map<String, Json> =
                serde_json::from_slice(&std::fs::read(base.join("queue-fixture.json")).unwrap())
                    .unwrap();
            Ok(config.into_iter().collect())
        })
        .withProviders(vec![Box::new(Providers)])
        .withEvents(DiscoverEvents::new(&[listeners]))
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
    assert_eq!(application(&root).handleCommand(&args).await.unwrap(), 0);
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
    let output = run(&root.0, &["queue:work", "--json", "--stop-when-empty"]).await;
    let rows = json_lines(&output);
    assert!(
        rows.iter()
            .any(|row| row["job"] == "terminal" && row["status"] == "failed")
    );
    assert_eq!(queue.size(None).await.unwrap(), 0);
    assert_eq!(db.table("failed_jobs").count("*").await.unwrap(), 2);
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
