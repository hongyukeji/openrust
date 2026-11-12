use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    bus::{
        Batch,
        events::{BatchCanceled, BatchDispatched, BatchFinished, BatchStarted},
    },
    cache::CacheManager,
    contracts::{
        cache::LockProvider as _,
        container::Container as _,
        events::{Event as EventPayload, Listener},
        foundation::Application as _,
        http::Kernel as _,
        queue::JobHandler,
    },
    database::{Connection, DB, DatabaseManager, Value, query::Record},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{
        JobContext, Payload, QueueError, QueueFuture, QueueResult,
        middleware::{JobMiddleware, SkipIfBatchCancelled},
    },
    redis::{RedisManager, connections::Connection as RedisConnection},
    routing::{Router, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Bus, Event, Queue},
    },
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    process::Command,
    time::timeout,
};

#[path = "../database/migrations/mod.rs"]
mod migrations;

struct Provider;

impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().instance(Arc::new(
                openrust::database::migrations::MigrationRegistry(migrations::ENTRIES),
            ))?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Queue::register("reports.generate", Arc::new(Probe))?;
            register_callbacks()?;
            for name in [
                BatchDispatched::NAME,
                BatchStarted::NAME,
                BatchFinished::NAME,
                BatchCanceled::NAME,
            ] {
                Event::listen(name, Arc::new(BatchObserver), Delivery::Sync)?;
            }
            Ok(())
        })
    }
}

struct BatchObserver;

impl Listener for BatchObserver {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let (batch, cause) = match event.name() {
                BatchDispatched::NAME => {
                    let value = event.downcast_ref::<BatchDispatched>().unwrap();
                    assert_eq!(value.batch.pending_jobs, value.batch.total_jobs);
                    (&value.batch, None)
                }
                BatchStarted::NAME => {
                    let value = event.downcast_ref::<BatchStarted>().unwrap();
                    assert!(value.batch.processedJobs() != 0 || value.batch.hasFailures());
                    (&value.batch, None)
                }
                BatchFinished::NAME => {
                    let value = event.downcast_ref::<BatchFinished>().unwrap();
                    assert_eq!(value.batch.pending_jobs, 0);
                    assert!(value.batch.finished());
                    (&value.batch, None)
                }
                BatchCanceled::NAME => {
                    let value = event.downcast_ref::<BatchCanceled>().unwrap();
                    let wire = serde_json::to_vec(value).unwrap();
                    let decoded: BatchCanceled = serde_json::from_slice(&wire).unwrap();
                    assert_eq!(decoded.batch, value.batch);
                    assert_eq!(decoded.exception, value.exception);
                    assert!(value.batch.cancelled());
                    (&value.batch, value.exception.as_deref())
                }
                _ => unreachable!(),
            };
            assert!(Bus::findBatch(&batch.id).await.unwrap().is_some());
            record_callback(batch, event.name(), cause).await.unwrap();
            Ok(None)
        })
    }
}

fn register_callbacks() -> QueueResult<()> {
    Bus::registerBatchCallback("reports.before", |batch, failure| {
        Box::pin(record_callback(batch, "before", failure))
    })?;
    Bus::registerBatchCallback("reports.progress", |batch, failure| {
        Box::pin(record_callback(batch, "progress", failure))
    })?;
    Bus::registerBatchCallback("reports.then", |batch, failure| {
        Box::pin(record_callback(batch, "then", failure))
    })?;
    Bus::registerBatchCallback("reports.catch", |batch, failure| {
        Box::pin(record_callback(batch, "catch", failure))
    })?;
    Bus::registerBatchCallback("reports.finally", |batch, failure| {
        Box::pin(record_callback(batch, "finally", failure))
    })?;
    Bus::registerBatchCallback("reports.failure", |batch, failure| {
        Box::pin(record_callback(batch, "failure", failure))
    })?;
    Bus::registerBatchCallback("reports.broken", |_, _| {
        Box::pin(async { Err(QueueError::InvalidPayload) })
    })?;
    Ok(())
}

async fn record_callback(batch: &Batch, phase: &str, failure: Option<&str>) -> QueueResult<()> {
    if phase == "before" {
        assert_eq!(batch.total_jobs, 0);
        assert!(Bus::findBatch(&batch.id).await?.is_none());
    }
    DB::connection(None)
        .unwrap()
        .statement(
            "insert into batch_callbacks (batch_id, phase, failure) values (?, ?, ?)",
            &[
                batch.id.as_str().into(),
                phase.into(),
                failure.map_or(Value::Null, Value::from),
            ],
        )
        .await
        .unwrap();
    Ok(())
}

async fn trace(db: &Connection, batch: &str) -> Vec<(String, Option<String>)> {
    db.table("batch_callbacks")
        .r#where("batch_id", "=", batch)
        .orderBy("id", "asc")
        .get()
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            let Some(Value::Text(phase)) = row.get("phase") else {
                panic!("missing phase")
            };
            let failure = match row.get("failure") {
                Some(Value::Text(value)) => Some(value.clone()),
                Some(Value::Null) => None,
                _ => panic!("invalid failure"),
            };
            (phase.clone(), failure)
        })
        .collect()
}

async fn callback_trace(db: &Connection, batch: &str) -> Vec<(String, Option<String>)> {
    trace(db, batch)
        .await
        .into_iter()
        .filter(|(name, _)| !name.starts_with("Batch"))
        .collect()
}

async fn event_trace(db: &Connection, batch: &str) -> Vec<(String, Option<String>)> {
    trace(db, batch)
        .await
        .into_iter()
        .filter(|(name, _)| name.starts_with("Batch"))
        .collect()
}

struct Probe;

impl JobHandler for Probe {
    fn middleware(&self) -> Vec<Arc<dyn JobMiddleware>> {
        vec![Arc::new(SkipIfBatchCancelled)]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            assert!(payload.batchId().is_some());
            let data = payload.data().as_str().unwrap();
            let db = DB::connection(None).unwrap();
            db.statement("insert into receipts (name) values (?)", &[data.into()])
                .await
                .unwrap();
            if data == "loader" {
                let batch = Bus::findBatch(payload.batchId().unwrap()).await?.unwrap();
                let loaded = Payload::new("reports.generate", "loaded")?;
                batch.add([loaded.clone(), loaded]).await?;
            }
            if data == "detail" && context.attempts() == 1 {
                return Err(QueueError::InvalidPayload);
            }
            if data == "failed"
                && db
                    .table("receipts")
                    .r#where("name", "=", "allow")
                    .count("*")
                    .await
                    .unwrap()
                    == 0
            {
                return Err(QueueError::InvalidPayload);
            }
            Ok(())
        })
    }
}

fn route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = request.route("kind").unwrap();
        if kind == "cancel-explicit" {
            let job = Payload::new("reports.generate", "skipped")?;
            let batch = Bus::batch([job]).onQueue("explicit").dispatch().await?;
            Bus::cancelBatch(&batch.id).await?;
            Bus::cancelBatch(&batch.id).await?;
            return Response::json(StatusCode::OK, &batch);
        }
        let jobs = |names: &[&str]| {
            names
                .iter()
                .map(|name| Payload::new("reports.generate", name))
                .collect::<QueueResult<Vec<_>>>()
        };
        let batch = match kind {
            "success" => Bus::batch(jobs(&["summary", "detail"])?)
                .name("monthly reports")
                .onQueue("batch")
                .before("reports.before")
                .progress("reports.broken")
                .progress("reports.progress")
                .then("reports.then")
                .finally("reports.finally"),
            "allow" => Bus::batch(jobs(&["after", "failed"])?)
                .name("allow")
                .onQueue("batch")
                .progress("reports.progress")
                .onFailure("reports.failure")
                .catch("reports.catch")
                .then("reports.then")
                .finally("reports.finally"),
            "cancel" => Bus::batch(jobs(&["failed", "skipped"])?)
                .name("cancel")
                .onQueue("batch")
                .catch("reports.catch")
                .finally("reports.finally"),
            "rollback" => Bus::batch(jobs(&["first", "second"])?).name("rollback"),
            "append-loader" => Bus::batch(jobs(&["loader"])?).onQueue("append"),
            "append-start" => Bus::batch(jobs(&["initial-append"])?).onQueue("append"),
            "redis-batch" => Bus::batch(jobs(&["redis-first", "redis-second"])?)
                .onConnection("redis")
                .onQueue("redis-batch"),
            "missing-callback" => Bus::batch(jobs(&["summary"])?).then("reports.missing"),
            "sync" => Bus::batch(jobs(&["skipped"])?).onConnection("sync"),
            _ => unreachable!(),
        }
        .dispatch()
        .await?;
        Ok(Response::json(StatusCode::OK, &batch).unwrap())
    })
}

fn append_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = request.route("id").unwrap();
        let data = request.route("data").unwrap();
        if let Some(batch) = Bus::findBatch(id).await?
            && let Some(updated) = batch.add([Payload::new("reports.generate", data)?]).await?
        {
            return Response::json(StatusCode::OK, &updated);
        }
        Ok(Response::empty(StatusCode::NOT_FOUND))
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/batch/{kind}", route)?;
    router.get("/batch/append/{id}/{data}", append_route)?;
    Ok(router)
}

fn application(base: &Path) -> Application {
    Application::configure(base.to_owned())
        .withConfig(|base, _| {
            let settings: serde_json::Map<String, Json> =
                serde_json::from_slice(&std::fs::read(base.join("settings.json")).unwrap())
                    .unwrap();
            Ok(settings.into_iter().collect())
        })
        .withProviders(vec![Box::new(Provider)])
        .withRouting(Routing::new(routes))
        .create()
        .unwrap()
}

#[tokio::test]
#[ignore = "Spawned by the batch integration test"]
async fn job_batches_artisan_process() {
    let base = PathBuf::from(std::env::var_os("OPENRUST_BATCH_FIXTURE").unwrap());
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("OPENRUST_BATCH_ARGS").unwrap()).unwrap();
    let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
    let status = std::env::var("OPENRUST_BATCH_STATUS")
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert_eq!(
        application(&base).handleCommand(&args).await.unwrap(),
        status
    );
}

async fn artisan(base: &Path, args: &[&str]) -> String {
    artisan_status(base, args, 0).await
}

async fn artisan_status(base: &Path, args: &[&str], status: i32) -> String {
    let output = timeout(
        Duration::from_secs(20),
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "job_batches_artisan_process",
                "--ignored",
                "--nocapture",
            ])
            .env("OPENRUST_BATCH_FIXTURE", base)
            .env("OPENRUST_BATCH_STATUS", status.to_string())
            .env("OPENRUST_BATCH_ARGS", serde_json::to_string(args).unwrap())
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8([output.stdout, output.stderr].concat()).unwrap()
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(address: std::net::SocketAddr, kind: &str, status: u16) -> Option<Json> {
    timeout(Duration::from_secs(10), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!(
                    "GET /batch/{kind} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status} ")),
            "{response}"
        );
        (status == 200)
            .then(|| serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap())
    })
    .await
    .unwrap()
}

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn exercise(mysql: Option<&str>) {
    let root = Root(std::env::temp_dir().join(format!("openrust-job-batches-{}", uuid_suffix())));
    std::fs::create_dir(&root.0).unwrap();
    std::fs::File::create(root.0.join("batch.sqlite")).unwrap();
    let connection = mysql.map_or_else(
        || json!({"driver":"sqlite","database":"batch.sqlite"}),
        |url| json!({"url":url}),
    );
    let redis_port = std::env::var("OPENRUST_TEST_REDIS_PORT")
        .ok()
        .map(|port| port.parse::<u16>().unwrap());
    assert!(redis_port.is_some() || std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none());
    let redis_prefix = format!("openrust-batch-test:{}:", uuid_suffix());
    let settings = json!({
        "database": {"default":"batch", "connections":{"batch":connection},
            "redis":{"default":{"port":redis_port.unwrap_or(6379)},"options":{"prefix":redis_prefix}}},
        "logging": {"default":"single","channels":{"single":{"driver":"single","path":"batch.log","locking":true}}},
        "cache": {"default":"database","stores":{"database":{"driver":"database"}}},
        "queue": {
            "default":"database",
            "connections": {"database":{"driver":"database","retry_after":90},"redis":{"driver":"redis","connection":"default","retry_after":90}},
            "failed": {"driver":"database-uuids","database":"batch"},
            "batching": {"database":"batch","table":"job_batches"}
        }
    });
    std::fs::write(root.0.join("settings.json"), settings.to_string()).unwrap();
    let mut app = application(&root.0);
    app.boot().await.unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let mut schema = db.getSchemaBuilder();
    for table in [
        "pending_ack_block",
        "job_batches_additions",
        "job_batches_outbox",
        "job_batches_jobs",
        "job_batches",
        "failed_jobs",
        "jobs",
        "cache_locks",
        "cache",
        "sessions",
        "password_reset_tokens",
        "users",
        "migrations",
        "receipts",
        "batch_callbacks",
    ] {
        schema.dropIfExists(table).await.unwrap();
    }
    artisan(&root.0, &["migrate", "--force"]).await;
    db.statement("create table receipts (name varchar(255) not null)", &[])
        .await
        .unwrap();
    schema
        .create("batch_callbacks", |table| {
            table.id();
            table.string("batch_id", 36);
            table.string("phase", 32);
            table.string("failure", 255).nullable();
        })
        .await
        .unwrap();
    let cache = app
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(None)
        .unwrap();
    let redis = redis_port.map(|_| {
        app.container()
            .make::<RedisManager>()
            .unwrap()
            .connection(None)
            .unwrap()
    });
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
    let batches = openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    request(address, "sync", 500).await;
    request(address, "missing-callback", 500).await;
    assert_eq!(db.table("job_batches").count("*").await.unwrap(), 0);
    let success = request(address, "success", 200).await.unwrap();
    let id = success["id"].as_str().unwrap();
    assert_eq!(success["pendingJobs"], 2);
    assert_eq!(callback_trace(&db, id).await, [("before".into(), None)]);
    let replay = db
        .scalar("select payload from jobs order by id limit 1", &[])
        .await
        .unwrap();
    let once = [
        "queue:work",
        "database",
        "--queue=batch",
        "--once",
        "--tries=2",
    ];
    tokio::join!(artisan(&root.0, &once), artisan(&root.0, &once));
    artisan(
        &root.0,
        &[
            "queue:work",
            "database",
            "--queue=batch",
            "--stop-when-empty",
            "--tries=2",
        ],
    )
    .await;
    assert_eq!(batches.find(id).await.unwrap().unwrap().progress(), 100);
    db.statement("insert into jobs (queue,payload,attempts,reserved_at,available_at,created_at) values ('batch',?,0,null,0,0)", &[replay]).await.unwrap();
    artisan(
        &root.0,
        &["queue:work", "database", "--queue=batch", "--once"],
    )
    .await;
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "summary")
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "detail")
            .count("*")
            .await
            .unwrap(),
        2
    );

    let callbacks = callback_trace(&db, id).await;
    assert_eq!(
        callbacks
            .iter()
            .filter(|(phase, _)| phase == "progress")
            .count(),
        2
    );
    assert_eq!(callbacks.len(), 5);
    assert!(
        std::fs::read_to_string(root.0.join("batch.log"))
            .unwrap()
            .contains("Batch callback failed")
    );
    assert!(callbacks.contains(&("then".into(), None)));
    assert!(callbacks.contains(&("finally".into(), None)));
    let lifecycle = trace(&db, id).await;
    assert_eq!(
        lifecycle
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "before",
            "BatchDispatched",
            "BatchStarted",
            "progress",
            "progress",
            "BatchFinished",
            "then",
            "finally"
        ]
    );
    artisan(&root.0, &["queue:retry-batch"]).await;
    let missing = artisan_status(&root.0, &["queue:retry-batch", "missing", id], 1).await;
    assert!(
        missing.contains("Unable to find a batch with ID [missing]"),
        "{missing}"
    );
    assert!(
        missing.contains("does not contain any failed jobs"),
        "{missing}"
    );
    for kind in ["cancel", "allow"] {
        let dispatched = request(address, kind, 200).await.unwrap();
        let id = dispatched["id"].as_str().unwrap();
        artisan(
            &root.0,
            &[
                "queue:work",
                "database",
                "--queue=batch",
                "--stop-when-empty",
                "--tries=1",
            ],
        )
        .await;
        let batch = batches.find(id).await.unwrap().unwrap();
        assert_eq!(batch.cancelled(), kind == "cancel");
        assert_eq!(batch.failed_jobs, 1);
        assert_eq!(batch.pending_jobs, 1);
        let events = event_trace(&db, id).await;
        if kind == "cancel" {
            assert_eq!(
                events,
                [
                    ("BatchDispatched".into(), None),
                    ("BatchStarted".into(), None),
                    ("BatchCanceled".into(), Some("invalid queue payload".into())),
                ]
            );
        } else {
            assert_eq!(
                events,
                [
                    ("BatchDispatched".into(), None),
                    ("BatchStarted".into(), None)
                ]
            );
        }
        let before_retry = callback_trace(&db, id).await;
        let failure = Some("invalid queue payload".to_owned());
        if kind == "allow" {
            assert_eq!(
                before_retry,
                [
                    ("progress".into(), None),
                    ("progress".into(), failure.clone()),
                    ("failure".into(), failure.clone()),
                    ("catch".into(), failure.clone()),
                    ("finally".into(), None),
                ]
            );
        } else {
            assert_eq!(
                before_retry,
                [("catch".into(), failure), ("finally".into(), None)]
            );
        }
        if kind == "allow" {
            db.statement("insert into receipts (name) values ('allow')", &[])
                .await
                .unwrap();
            let retry = ["queue:retry-batch", id, id];
            let key = "framework/command-queue:retry-batch-4d4f75487783972613585397ef95e8cec57dd22ddf2dd1b13bbe23a245122f5f";
            let mut lock = cache
                .lock(key, Some(Duration::from_secs(60)), None)
                .unwrap();
            assert!(lock.get().await.unwrap());
            let locked = artisan_status(
                &root.0,
                &["queue:retry-batch", "isolated-probe", "--isolated=12"],
                12,
            )
            .await;
            assert!(locked.contains("command is already running"), "{locked}");
            assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
            assert!(lock.release().await.unwrap());

            let reject_retry = if mysql.is_some() {
                r#"alter table jobs add constraint reject_retry check (payload not like '%"failed"%')"#
            } else {
                r#"create trigger reject_retry before insert on jobs when new.payload like '%"failed"%' begin select raise(abort, 'retry rejected'); end"#
            };
            db.unprepared(reject_retry).await.unwrap();
            artisan_status(&root.0, &["queue:retry-batch", id, "--isolated"], 1).await;
            assert_eq!(db.table("cache_locks").count("*").await.unwrap(), 0);
            assert_eq!(
                db.table("failed_jobs")
                    .r#where("uuid", "=", batch.failed_job_ids[0].as_str())
                    .count("*")
                    .await
                    .unwrap(),
                1
            );
            let allow_retry = if mysql.is_some() {
                "alter table jobs drop check reject_retry"
            } else {
                "drop trigger reject_retry"
            };
            db.unprepared(allow_retry).await.unwrap();

            let mixed = ["queue:retry-batch", "missing", id, id];
            let (left, right) =
                tokio::join!(artisan_status(&root.0, &mixed, 1), artisan(&root.0, &retry));
            assert!(
                left.contains("Unable to find a batch with ID [missing]"),
                "{left}"
            );
            assert_eq!(
                [&left, &right]
                    .iter()
                    .map(|line| line.matches("Queued failed job").count())
                    .sum::<usize>(),
                1
            );
            assert_eq!(db.table("jobs").count("*").await.unwrap(), 1);
            let repeat = artisan(&root.0, &retry).await;
            assert!(repeat.contains("no longer available to retry"), "{repeat}");
            assert_eq!(db.table("jobs").count("*").await.unwrap(), 1);
            artisan(
                &root.0,
                &["queue:work", "database", "--queue=batch", "--once"],
            )
            .await;
            let retried = batches.find(id).await.unwrap().unwrap();
            assert!(retried.finished());
            assert_eq!(retried.progress(), 100);
            assert_eq!(retried.failed_jobs, 1);
            assert!(retried.failed_job_ids.is_empty());
            assert_eq!(
                &callback_trace(&db, id).await[before_retry.len()..],
                [("progress".into(), None), ("then".into(), None),]
            );
        }
    }
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "skipped")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "after")
            .count("*")
            .await
            .unwrap(),
        1
    );

    let reject_second = if mysql.is_some() {
        r#"alter table jobs add constraint reject_second check (payload not like '%"second"%')"#
    } else {
        r#"create trigger reject_second before insert on jobs when new.payload like '%"second"%' begin select raise(abort, 'second job rejected'); end"#
    };
    db.unprepared(reject_second).await.unwrap();
    request(address, "rollback", 500).await;
    assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
    assert_eq!(db.table("job_batches").count("*").await.unwrap(), 3);
    assert_eq!(db.table("job_batches_jobs").count("*").await.unwrap(), 7);
    verify_pruning(&root.0, &db).await;
    let explicit = request(address, "cancel-explicit", 200).await.unwrap();
    let id = explicit["id"].as_str().unwrap();
    assert_eq!(
        event_trace(&db, id).await,
        [
            ("BatchDispatched".into(), None),
            ("BatchCanceled".into(), None)
        ]
    );
    artisan(
        &root.0,
        &["queue:work", "database", "--queue=explicit", "--once"],
    )
    .await;
    assert_eq!(
        event_trace(&db, id).await,
        [
            ("BatchDispatched".into(), None),
            ("BatchCanceled".into(), None),
            ("BatchStarted".into(), None),
            ("BatchFinished".into(), None),
        ]
    );
    if let Some(redis) = redis {
        verify_redis_batch(&root.0, &db, &redis, address).await;
    }
    verify_database_append(&root.0, &db, address).await;
    server.abort();
}

async fn verify_database_append(base: &Path, db: &Connection, address: std::net::SocketAddr) {
    request(address, "append/missing/report", 404).await;
    let initial = request(address, "append-start", 200).await.unwrap();
    let id = initial["id"].as_str().unwrap();
    let first = format!("append/{id}/first-extra");
    let second = format!("append/{id}/second-extra");
    let (first, second) = tokio::join!(
        request(address, &first, 200),
        request(address, &second, 200)
    );
    let mut counts = [
        first.unwrap()["totalJobs"].as_u64().unwrap(),
        second.unwrap()["totalJobs"].as_u64().unwrap(),
    ];
    counts.sort_unstable();
    assert_eq!(counts, [2, 3]);
    request(address, &format!("append/{id}/second"), 500).await;
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    assert_eq!(repository.find(id).await.unwrap().unwrap().pending_jobs, 3);
    let work = [
        "queue:work",
        "database",
        "--queue=append",
        "--stop-when-empty",
    ];
    artisan(base, &work).await;
    assert!(repository.find(id).await.unwrap().unwrap().finished());
    let reopened = request(address, &format!("append/{id}/late-extra"), 200)
        .await
        .unwrap();
    assert_eq!(reopened["totalJobs"], 4);
    assert_eq!(reopened["pendingJobs"], 1);
    assert!(reopened["finishedAt"].is_null());
    artisan(base, &work).await;
    assert_eq!(repository.find(id).await.unwrap().unwrap().progress(), 100);
    assert_eq!(
        event_trace(db, id)
            .await
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        [
            "BatchDispatched",
            "BatchStarted",
            "BatchFinished",
            "BatchFinished"
        ]
    );
    let loader = request(address, "append-loader", 200).await.unwrap();
    let loader_id = loader["id"].as_str().unwrap();
    artisan(base, &work).await;
    let loaded = repository.find(loader_id).await.unwrap().unwrap();
    assert_eq!(loaded.total_jobs, 3);
    assert_eq!(loaded.progress(), 100);
    assert_eq!(
        db.table("receipts")
            .r#where("name", "=", "loaded")
            .count("*")
            .await
            .unwrap(),
        2
    );
}

async fn verify_redis_batch(
    base: &Path,
    db: &Connection,
    redis: &RedisConnection,
    address: std::net::SocketAddr,
) {
    let ready = redis.key("queues:redis-batch");
    redis
        .set("queues:redis-batch", b"reject publication")
        .await
        .unwrap();
    let accepted = request(address, "redis-batch", 200).await.unwrap();
    let id = accepted["id"].as_str().unwrap();
    assert_eq!(accepted["pendingJobs"], 2);
    assert_eq!(db.table("job_batches_outbox").count("*").await.unwrap(), 1);
    assert!(event_trace(db, id).await.is_empty());
    artisan_status(base, &["queue:publish-batches", "--limit=1"], 1).await;
    assert_eq!(
        redis.get("queues:redis-batch").await.unwrap().unwrap(),
        b"reject publication"
    );
    redis.command("DEL", &[ready.as_bytes()]).await.unwrap();

    // Redis accepts both jobs, but a failed SQL acknowledgement leaves the intent for replay.
    let mut schema = db.getSchemaBuilder();
    schema
        .create("pending_ack_block", |table| {
            table.string("batch_id", 255).primary();
            table
                .foreign(&["batch_id"], None)
                .references("batch_id")
                .on("job_batches_outbox");
        })
        .await
        .unwrap();
    db.table("pending_ack_block")
        .insert(&[[("batch_id".into(), id.into())].into_iter().collect()])
        .await
        .unwrap();
    artisan_status(base, &["queue:publish-batches"], 1).await;
    assert_eq!(db.table("job_batches_outbox").count("*").await.unwrap(), 1);
    let marker = format!("queue-batches:{id}");
    assert_eq!(redis.get(&marker).await.unwrap(), Some(b"1".to_vec()));
    schema.dropIfExists("pending_ack_block").await.unwrap();
    let args = ["queue:publish-batches", "--limit=1"];
    let (left, right) = tokio::join!(artisan(base, &args), artisan(base, &args));
    assert_eq!(
        [left, right]
            .iter()
            .filter(|output| output.contains("1 Redis batches published."))
            .count(),
        1
    );
    assert_eq!(db.table("job_batches_outbox").count("*").await.unwrap(), 0);
    assert_eq!(
        redis.command("LLEN", &[ready.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(2)
    );
    artisan(
        base,
        &[
            "queue:work",
            "redis",
            "--queue=redis-batch",
            "--stop-when-empty",
        ],
    )
    .await;
    for name in ["redis-first", "redis-second"] {
        assert_eq!(
            db.table("receipts")
                .r#where("name", "=", name)
                .count("*")
                .await
                .unwrap(),
            1
        );
    }
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    assert_eq!(repository.find(id).await.unwrap().unwrap().progress(), 100);
    assert_eq!(
        event_trace(db, id).await,
        [
            ("BatchDispatched".into(), None),
            ("BatchStarted".into(), None),
            ("BatchFinished".into(), None),
        ]
    );
    let marker = redis.key(&marker);
    redis.command("DEL", &[marker.as_bytes()]).await.unwrap();

    let normal = request(address, "redis-batch", 200).await.unwrap();
    let normal_id = normal["id"].as_str().unwrap();
    assert_eq!(db.table("job_batches_outbox").count("*").await.unwrap(), 0);
    artisan(
        base,
        &[
            "queue:work",
            "redis",
            "--queue=redis-batch",
            "--stop-when-empty",
        ],
    )
    .await;
    assert_eq!(
        repository
            .find(normal_id)
            .await
            .unwrap()
            .unwrap()
            .progress(),
        100
    );
    redis
        .set("queues:redis-batch", b"reject additions")
        .await
        .unwrap();
    for name in ["redis-extra-one", "redis-extra-two"] {
        request(address, &format!("append/{normal_id}/{name}"), 200)
            .await
            .unwrap();
    }
    assert_eq!(
        db.table("job_batches_additions").count("*").await.unwrap(),
        2
    );
    let additions = db.table("job_batches_additions").get().await.unwrap();
    redis.command("DEL", &[ready.as_bytes()]).await.unwrap();
    let recovered = artisan(base, &["queue:publish-batches"]).await;
    assert!(
        recovered.contains("2 Redis batches published."),
        "{recovered}"
    );
    assert_eq!(
        db.table("job_batches_additions").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        redis.command("LLEN", &[ready.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(2)
    );
    artisan(
        base,
        &[
            "queue:work",
            "redis",
            "--queue=redis-batch",
            "--stop-when-empty",
        ],
    )
    .await;
    let complete = repository.find(normal_id).await.unwrap().unwrap();
    assert_eq!(complete.total_jobs, 4);
    assert_eq!(complete.progress(), 100);
    assert_eq!(
        event_trace(db, normal_id)
            .await
            .iter()
            .filter(|(name, _)| name == "BatchDispatched")
            .count(),
        1
    );
    for row in additions {
        let Some(Value::Text(id)) = row.get("id") else {
            panic!("missing addition id")
        };
        let marker = redis.key(&format!("queue-batches:{id}"));
        redis.command("DEL", &[marker.as_bytes()]).await.unwrap();
    }
    let marker = redis.key(&format!("queue-batches:{normal_id}"));
    redis.command("DEL", &[marker.as_bytes()]).await.unwrap();
}

async fn verify_pruning(base: &Path, db: &Connection) {
    let timestamp = chrono::Utc::now().timestamp();
    let at = |hours: i64| Value::from(timestamp - hours * 3600);
    let batch = |id: String, created, finished, cancelled| -> Record {
        [
            ("id".into(), id.into()),
            ("name".into(), "prune probe".into()),
            ("total_jobs".into(), 1_i64.into()),
            ("pending_jobs".into(), 0_i64.into()),
            ("failed_jobs".into(), 0_i64.into()),
            ("failed_job_ids".into(), "[]".into()),
            ("created_at".into(), created),
            ("finished_at".into(), finished),
            ("cancelled_at".into(), cancelled),
        ]
        .into_iter()
        .collect()
    };
    let completed = (0..1001)
        .map(|i| batch(format!("old-{i:04}"), at(80), at(25), Value::Null))
        .collect::<Vec<_>>();
    for chunk in completed.chunks(50) {
        db.table("job_batches").insert(chunk).await.unwrap();
    }
    for (id, created, finished, cancelled) in [
        ("recent-completed", at(40), at(2), Value::Null),
        ("old-pending", at(80), Value::Null, Value::Null),
        ("recent-pending", at(2), Value::Null, Value::Null),
        ("old-cancelled", at(40), at(2), at(2)),
        ("recent-cancelled", at(2), at(1), at(1)),
    ] {
        db.table("job_batches")
            .insert(&[batch(id.into(), created, finished, cancelled)])
            .await
            .unwrap();
    }
    for id in ["old-0000", "old-1000", "old-cancelled", "recent-completed"] {
        db.table("job_batches_jobs")
            .insert(&[[
                ("batch_id".into(), id.into()),
                ("job_id".into(), "job".into()),
                ("delivery_id".into(), "".into()),
                ("succeeded".into(), true.into()),
            ]
            .into_iter()
            .collect()])
            .await
            .unwrap();
    }
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    let default = artisan(base, &["queue:prune-batches"]).await;
    assert!(default.contains("1001 entries deleted."), "{default}");
    assert!(repository.find("old-1000").await.unwrap().is_none());
    for id in [
        "recent-completed",
        "old-pending",
        "recent-pending",
        "old-cancelled",
        "recent-cancelled",
    ] {
        assert!(repository.find(id).await.unwrap().is_some(), "{id}");
    }
    db.table("job_batches")
        .insert(&[
            batch("outside-custom".into(), at(80), at(49), Value::Null),
            batch("inside-custom".into(), at(80), at(30), Value::Null),
        ])
        .await
        .unwrap();
    let args = [
        "queue:prune-batches",
        "--hours=48",
        "--unfinished=72",
        "--cancelled=36",
    ];
    let custom = artisan(base, &args).await;
    assert!(custom.contains("1 entries deleted."), "{custom}");
    assert!(custom.contains("1 unfinished entries deleted."), "{custom}");
    assert!(custom.contains("1 cancelled entries deleted."), "{custom}");
    for id in ["old-pending", "old-cancelled", "outside-custom"] {
        assert!(repository.find(id).await.unwrap().is_none(), "{id}");
    }
    for id in [
        "recent-completed",
        "recent-pending",
        "recent-cancelled",
        "inside-custom",
    ] {
        assert!(repository.find(id).await.unwrap().is_some(), "{id}");
    }
    assert_eq!(db.table("job_batches_jobs").count("*").await.unwrap(), 8);
    let repeat = artisan(base, &args).await;
    for label in [
        "entries deleted.",
        "unfinished entries deleted.",
        "cancelled entries deleted.",
    ] {
        assert!(repeat.contains(&format!("0 {label}")), "{repeat}");
    }
}

fn uuid_suffix() -> String {
    Payload::new("reports.generate", ()).unwrap().uuid().into()
}

#[tokio::test]
async fn dispatches_and_tracks_batches_on_sqlite() {
    exercise(None).await;
}

#[tokio::test]
async fn dispatches_and_tracks_batches_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    exercise(Some(&url)).await;
}
