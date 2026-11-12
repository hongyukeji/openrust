use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    bus::{
        Batch, PendingBatch, PendingChain,
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
        Delay, DispatchResult, JobContext, Payload, QueueError, QueueFuture, QueueManager,
        QueueResult,
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
            Bus::registerChainCallback("chain.batch-failed", |job, error| {
                Box::pin(async move {
                    assert!(matches!(error, QueueError::ChainedBatchFailed(_)));
                    let data = job.data().as_str().unwrap();
                    record_chain_receipt(&format!("{data}.catch")).await;
                    Ok(())
                })
            })?;
            Queue::register("chain.step", Arc::new(ChainProbe))?;
            Bus::registerChainCallback("chain.failed", |job, error| {
                Box::pin(async move {
                    assert!(matches!(error, QueueError::InvalidPayload));
                    let data = job.data().as_str().unwrap();
                    record_chain_receipt(&format!("{data}.catch")).await;
                    Ok(())
                })
            })?;
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
            if data.ends_with(".edit-batched") {
                let added = Payload::new("reports.generate", format!("{data}-added"))?;
                context.prependToChain(added)?;
            }
            if data == "loader" {
                let batch = Bus::findBatch(payload.batchId().unwrap()).await?.unwrap();
                let loaded = Payload::new("reports.generate", "loaded")?;
                batch.add([loaded.clone(), loaded]).await?;
            }
            if data.ends_with(".fatal")
                || data.ends_with(".stop") && chain_receipt(&db, data).await == 1
                || data == "detail" && context.attempts() == 1
            {
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

fn edit_chain(context: &mut JobContext<'_>, before: Payload, after: Payload) -> QueueResult<()> {
    context.prependToChain(before)?;
    context.appendToChain(after)
}

struct ChainProbe;

impl JobHandler for ChainProbe {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let data = payload.data().as_str().unwrap();
            let (connection, step) = data.split_once('.').unwrap();
            assert_eq!(context.connection(), connection);
            assert_eq!(context.queue(), "chain");
            record_chain_receipt(data).await;
            let step_job = |name: &str| Payload::new("chain.step", format!("{connection}.{name}"));
            if matches!(
                step,
                "edit-retry" | "edit-release" | "edit-fail" | "edit-last"
            ) {
                let before = step_job(&format!("{step}-before"))?;
                let after = step_job(&format!("{step}-after"))?;
                edit_chain(context, before, after)?;
            }
            if step == "edit-batch" {
                let report = Payload::new(
                    "reports.generate",
                    format!("{connection}.edit-batch-report"),
                )?;
                let batch = Bus::batch([report])
                    .name(format!("{connection}.edit-batch"))
                    .onQueue("inner-batch");
                context.prependToChain(batch)?;
            }
            match step {
                "retry" | "edit-retry" if context.attempts() == 1 => {
                    Err(QueueError::InvalidPayload)
                }
                "fail" | "edit-fail" => Err(QueueError::InvalidPayload),
                "release" | "edit-release" if context.attempts() == 1 => {
                    context.release(Duration::ZERO);
                    Ok(())
                }
                _ => Ok(()),
            }
        })
    }
}

async fn record_chain_receipt(data: &str) {
    DB::connection(None)
        .unwrap()
        .statement("insert into receipts (name) values (?)", &[data.into()])
        .await
        .unwrap();
}

fn chain_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let connection = request.route("connection").unwrap();
        let kind = request.route("kind").unwrap();
        let step = |name: &str| Payload::new("chain.step", format!("{connection}.{name}"));
        let chain = Bus::chain([step(kind)?])
            .onConnection(connection)
            .onQueue("chain")
            .catch("chain.failed");
        let chain = if kind == "edit-last" {
            chain
        } else {
            chain.prepend(step("first")?).append(step("last")?)
        };
        let chain = match kind {
            "missing-callback" => chain.catch("missing"),
            "delay" => chain.delay(Delay::For(Duration::from_secs(3600))),
            _ => chain,
        };
        let result = chain.dispatch().await?;
        let queued = matches!(result, DispatchResult::Queued(_));
        Response::json(StatusCode::OK, json!({ "queued": queued }))
    })
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

fn batched_chains(left: [Payload; 2], right: [Payload; 2]) -> PendingBatch {
    Bus::batch([]).addChain(left).addChain(right)
}

fn batched_chain_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let connection = request.route("connection").unwrap();
        let kind = request.route("kind").unwrap();
        let step = |name| Payload::new("reports.generate", format!("{connection}.{name}"));
        let batch = match kind {
            "empty" => Bus::batch([step("unused")?]).addChain([]),
            "duplicate" => {
                let job = step("duplicate")?;
                Bus::batch([job.clone()]).addChain([job])
            }
            "stop" => Bus::batch([])
                .addChain([step("stop")?, step("blocked")?])
                .allowFailures(),
            "recover" => Bus::batch([]).addChain([step("recover-head")?, step("recover-tail")?]),
            "edit" => Bus::batch([]).addChain([step("edit-batched")?, step("edit-batched-tail")?]),
            _ => batched_chains(
                [step("left-first")?, step("left-tail")?],
                [step("right-first")?, step("right-tail")?],
            ),
        }
        .onConnection(connection)
        .onQueue("batched-chains")
        .then("reports.then")
        .dispatch()
        .await?;
        Response::json(StatusCode::OK, batch)
    })
}

fn append_chain_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = request.route("id").unwrap();
        let Some(batch) = Bus::findBatch(id).await? else {
            return Ok(Response::empty(StatusCode::NOT_FOUND));
        };
        let step = |name| Payload::new("reports.generate", name);
        let updated = batch
            .addChain([step("added-first")?, step("added-tail")?])
            .await?;
        Response::json(StatusCode::OK, updated)
    })
}

fn release_chain(first: Payload, batch: PendingBatch, last: Payload) -> QueueResult<PendingChain> {
    Ok(Bus::chain([first]).appendBatch(batch)?.append(last))
}

fn chained_batch_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let connection = request.route("connection").unwrap();
        let kind = request.route("kind").unwrap();
        let step = |name: &str| Payload::new("chain.step", format!("{connection}.{name}"));
        let batch_job =
            |name| Payload::new("reports.generate", format!("{connection}.{kind}-{name}"));
        let fatal = Payload::new("reports.generate", format!("{connection}.{kind}.fatal"))?;
        let jobs = match kind {
            "deny" | "allow" => [fatal, batch_job("right")?],
            _ => [batch_job("left")?, batch_job("right")?],
        };
        let name = format!("chained-{connection}-{kind}");
        let mut batch = Bus::batch(jobs)
            .name(&name)
            .onQueue("inner-batch")
            .finally("reports.finally");
        if kind == "allow" {
            batch = batch.allowFailures();
        }
        if kind == "cross" {
            batch = batch.onConnection("database");
        }
        let first = step(format!("{kind}-first").as_str())?;
        let last = step(format!("{kind}-last").as_str())?;
        let chain = if kind == "pair" {
            let second = Bus::batch([batch_job("second")?])
                .name(format!("{name}-second"))
                .onQueue("inner-batch");
            Bus::chain([])
                .prependBatch(batch)?
                .appendBatch(second)?
                .append(last)
        } else {
            release_chain(first, batch, last)?
        };
        chain
            .onConnection(connection)
            .onQueue("chain")
            .catch("chain.batch-failed")
            .dispatch()
            .await?;
        Response::json(StatusCode::OK, json!({"name": name}))
    })
}

fn cancel_chained_batch(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = request.route("id").unwrap();
        let Some(batch) = Bus::cancelBatch(id).await? else {
            return Ok(Response::empty(StatusCode::NOT_FOUND));
        };
        Response::json(StatusCode::OK, batch)
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/batch/{kind}", route)?;
    router.get("/batch/append/{id}/{data}", append_route)?;
    router.get("/batch/chain/{connection}/{kind}", chain_route)?;
    router.get(
        "/batch/batched-chains/{connection}/{kind}",
        batched_chain_route,
    )?;
    router.get("/batch/append-chain/{id}", append_chain_route)?;
    router.get(
        "/batch/chained-batch/{connection}/{kind}",
        chained_batch_route,
    )?;
    router.get("/batch/cancel-chained/{id}", cancel_chained_batch)?;
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
            "connections": {"sync":{"driver":"sync"},"database":{"driver":"database","retry_after":90},"redis":{"driver":"redis","connection":"default","retry_after":90}},
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
        "job_batches_continuations",
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
    let queue = app.container().make::<QueueManager>().unwrap();
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
        verify_chain(&root.0, &db, &queue, address, "redis").await;
        verify_chain_redis_handoff(&redis, &queue, address).await;
        verify_chain_edits(&root.0, &db, address, "redis").await;
        verify_batched_chain_edit(&root.0, &db, address, "redis", None).await;
        verify_batched_chains(&root.0, &db, &queue, address, "redis").await;
        verify_chained_batches(&root.0, &db, &queue, address, "redis", "cross").await;
        verify_chained_batch_recovery(&root.0, &db, address, Some(&redis), mysql.is_some()).await;
    }
    verify_chain(&root.0, &db, &queue, address, "database").await;
    verify_chain_database_handoff(&root.0, &db, &queue, address, mysql.is_some()).await;
    assert_eq!(
        request(address, "chain/sync/ok", 200).await.unwrap()["queued"],
        false
    );
    assert_eq!(chain_receipt(&db, "sync.ok").await, 1);
    assert_eq!(chain_receipt(&db, "sync.last").await, 1);
    request(address, "chain/sync/fail", 500).await;
    assert_eq!(chain_receipt(&db, "sync.fail.catch").await, 1);
    assert_eq!(chain_receipt(&db, "sync.last").await, 1);
    verify_database_append(&root.0, &db, address).await;
    verify_batched_chains(&root.0, &db, &queue, address, "database").await;
    verify_batched_chain_recovery(&root.0, &db, address, mysql.is_some()).await;
    request(address, "chained-batch/sync/ok", 500).await;
    assert_eq!(chain_receipt(&db, "sync.ok-first").await, 0);
    verify_chained_batches(&root.0, &db, &queue, address, "database", "ok").await;
    verify_chained_batch_recovery(&root.0, &db, address, None, mysql.is_some()).await;
    verify_chain_edits(&root.0, &db, address, "database").await;
    verify_batched_chain_edit(&root.0, &db, address, "database", Some(mysql.is_some())).await;
    request(address, "chain/sync/edit-last", 200).await;
    assert_eq!(chain_receipt(&db, "sync.edit-last-before").await, 1);
    assert_eq!(chain_receipt(&db, "sync.edit-last-after").await, 1);
    server.abort();
}

async fn chain_receipt(db: &Connection, name: &str) -> u64 {
    db.table("receipts")
        .r#where("name", "=", name)
        .count("*")
        .await
        .unwrap()
}

async fn verify_chain(
    base: &Path,
    db: &Connection,
    manager: &QueueManager,
    address: std::net::SocketAddr,
    connection: &str,
) {
    let queue = manager.connection(Some(connection)).unwrap();
    let route = |kind| format!("chain/{connection}/{kind}");
    let receipt = |step| format!("{connection}.{step}");
    let work = [
        "queue:work",
        connection,
        "--queue=chain",
        "--once",
        "--tries=2",
    ];
    request(address, &route("missing-callback"), 500).await;
    assert_eq!(queue.size(Some("chain")).await.unwrap(), 0);

    for (index, kind) in ["retry", "release"].into_iter().enumerate() {
        let completed = u64::try_from(index).unwrap();
        assert_eq!(
            request(address, &route(kind), 200).await.unwrap()["queued"],
            true
        );
        assert_eq!(queue.size(Some("chain")).await.unwrap(), 1);
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &receipt("first")).await, completed + 1);
        assert_eq!(chain_receipt(db, &receipt(kind)).await, 0);
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &receipt(kind)).await, 1);
        assert_eq!(chain_receipt(db, &receipt("last")).await, completed);
        assert_eq!(queue.size(Some("chain")).await.unwrap(), 1);
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &receipt(kind)).await, 2);
        assert_eq!(chain_receipt(db, &receipt("last")).await, completed);
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &receipt("last")).await, completed + 1);
        assert_eq!(queue.size(Some("chain")).await.unwrap(), 0);
    }

    request(address, &route("fail"), 200).await;
    artisan(base, &work).await;
    artisan(base, &work).await;
    assert_eq!(chain_receipt(db, &receipt("fail.catch")).await, 0);
    artisan(base, &work).await;
    assert_eq!(chain_receipt(db, &receipt("fail.catch")).await, 1);
    assert_eq!(chain_receipt(db, &receipt("last")).await, 2);
    assert_eq!(queue.size(Some("chain")).await.unwrap(), 0);
}

async fn verify_chain_database_handoff(
    base: &Path,
    db: &Connection,
    manager: &QueueManager,
    address: std::net::SocketAddr,
    mysql: bool,
) {
    request(address, "chain/database/handoff", 200).await;
    // Fail only insertion of the successor. The head is already reserved when this fires.
    let reject = if mysql {
        r#"alter table jobs add constraint reject_handoff check (payload not like '%"data":"database.handoff"%' or payload like '%"data":"database.first"%')"#
    } else {
        r#"create trigger reject_handoff before insert on jobs when new.payload like '%"data":"database.handoff"%' begin select raise(abort, 'chain handoff rejected'); end"#
    };
    db.unprepared(reject).await.unwrap();
    let work = [
        "queue:work",
        "database",
        "--queue=chain",
        "--once",
        "--tries=2",
    ];
    artisan_status(base, &work, 1).await;
    assert_eq!(
        db.table("jobs")
            .r#where("queue", "=", "chain")
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(chain_receipt(db, "database.handoff").await, 0);
    let allow = if mysql {
        "alter table jobs drop check reject_handoff"
    } else {
        "drop trigger reject_handoff"
    };
    db.unprepared(allow).await.unwrap();
    db.statement(
        "update jobs set reserved_at = 0 where queue = ?",
        &["chain".into()],
    )
    .await
    .unwrap();
    artisan(base, &work).await;
    artisan(base, &work).await;
    artisan(base, &work).await;
    assert_eq!(chain_receipt(db, "database.handoff").await, 1);
    assert_eq!(chain_receipt(db, "database.last").await, 3);
    request(address, "chain/database/delay", 200).await;
    let queue = manager.connection(Some("database")).unwrap();
    assert_eq!(queue.size(Some("chain")).await.unwrap(), 1);
    assert!(queue.pop_now(Some("chain")).await.unwrap().is_none());
    db.statement("delete from jobs where queue = ?", &["chain".into()])
        .await
        .unwrap();
}

async fn verify_chain_redis_handoff(
    redis: &RedisConnection,
    manager: &QueueManager,
    address: std::net::SocketAddr,
) {
    request(address, "chain/redis/handoff", 200).await;
    let queue = manager.connection(Some("redis")).unwrap();
    let mut reserved_job = queue.pop_now(Some("chain")).await.unwrap().unwrap();
    let ready = redis.key("queues:chain");
    let notify = format!("{ready}:notify");
    let reserved = format!("{ready}:reserved");
    redis
        .command("SET", &[notify.as_bytes(), b"bad type"])
        .await
        .unwrap();
    let next = Payload::new("chain.step", "redis.handoff").unwrap();
    assert!(reserved_job.advance_chain(&next).await.is_err());
    assert_eq!(
        redis
            .command("ZCARD", &[reserved.as_bytes()])
            .await
            .unwrap(),
        openrust::redis::Value::Int(1)
    );
    assert_eq!(
        redis.command("LLEN", &[ready.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(0)
    );
    redis.command("DEL", &[notify.as_bytes()]).await.unwrap();
    reserved_job.advance_chain(&next).await.unwrap();
    assert!(matches!(
        reserved_job.advance_chain(&next).await,
        Err(QueueError::LostReservation)
    ));
    let mut successor = queue.pop_now(Some("chain")).await.unwrap().unwrap();
    assert_eq!(successor.uuid(), next.uuid());
    assert_eq!(successor.attempts(), 1);
    successor.delete().await.unwrap();
    assert_eq!(queue.size(Some("chain")).await.unwrap(), 0);
}

async fn verify_batched_chains(
    base: &Path,
    db: &Connection,
    manager: &QueueManager,
    address: std::net::SocketAddr,
    connection: &str,
) {
    let queue = manager.connection(Some(connection)).unwrap();
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    let request_path = |kind| format!("batched-chains/{connection}/{kind}");
    request(address, &request_path("empty"), 500).await;
    request(address, &request_path("duplicate"), 500).await;
    assert_eq!(queue.size(Some("batched-chains")).await.unwrap(), 0);
    let batch = request(address, &request_path("start"), 200).await.unwrap();
    let id = batch["id"].as_str().unwrap();
    assert_eq!(batch["pendingJobs"], 4);
    assert_eq!(queue.size(Some("batched-chains")).await.unwrap(), 2);
    let once = ["queue:work", connection, "--queue=batched-chains", "--once"];
    artisan(base, &once).await;
    assert_eq!(
        chain_receipt(db, &format!("{connection}.left-first")).await,
        1
    );
    assert_eq!(
        chain_receipt(db, &format!("{connection}.left-tail")).await,
        0
    );
    assert_eq!(repository.find(id).await.unwrap().unwrap().pending_jobs, 3);
    artisan(base, &once).await;
    assert_eq!(
        chain_receipt(db, &format!("{connection}.right-first")).await,
        1
    );
    assert_eq!(
        chain_receipt(db, &format!("{connection}.right-tail")).await,
        0
    );
    tokio::join!(artisan(base, &once), artisan(base, &once));
    assert!(repository.find(id).await.unwrap().unwrap().finished());
    assert_eq!(
        chain_receipt(db, &format!("{connection}.left-tail")).await,
        1
    );
    assert_eq!(
        chain_receipt(db, &format!("{connection}.right-tail")).await,
        1
    );
    assert_eq!(callback_trace(db, id).await, [("then".into(), None)]);

    let added = request(address, &format!("append-chain/{id}"), 200)
        .await
        .unwrap();
    assert_eq!(added["totalJobs"], 6);
    assert_eq!(added["pendingJobs"], 2);
    assert_eq!(queue.size(Some("batched-chains")).await.unwrap(), 1);
    artisan(base, &once).await;
    assert_eq!(repository.find(id).await.unwrap().unwrap().pending_jobs, 1);
    artisan(base, &once).await;
    assert_eq!(repository.find(id).await.unwrap().unwrap().progress(), 100);

    let failed = request(address, &request_path("stop"), 200).await.unwrap();
    let failed_id = failed["id"].as_str().unwrap();
    artisan(base, &once).await;
    let failed = repository.find(failed_id).await.unwrap().unwrap();
    assert_eq!(failed.failed_jobs, 1);
    assert_eq!(failed.pending_jobs, 2);
    assert!(!failed.cancelled());
    assert_eq!(queue.size(Some("batched-chains")).await.unwrap(), 0);
    assert_eq!(chain_receipt(db, &format!("{connection}.blocked")).await, 0);
    artisan(base, &["queue:retry", &failed.failed_job_ids[0]]).await;
    artisan(base, &once).await;
    assert_eq!(chain_receipt(db, &format!("{connection}.blocked")).await, 0);
    artisan(base, &once).await;
    assert_eq!(chain_receipt(db, &format!("{connection}.blocked")).await, 1);
    assert!(
        repository
            .find(failed_id)
            .await
            .unwrap()
            .unwrap()
            .finished()
    );
}

async fn verify_batched_chain_recovery(
    base: &Path,
    db: &Connection,
    address: std::net::SocketAddr,
    mysql: bool,
) {
    let batch = request(address, "batched-chains/database/recover", 200)
        .await
        .unwrap();
    let id = batch["id"].as_str().unwrap();
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    let reject = if mysql {
        r#"alter table jobs add constraint reject_batched_tail check (payload not like '%"data":"database.recover-tail"%' or payload like '%"data":"database.recover-head"%')"#
    } else {
        r#"create trigger reject_batched_tail before insert on jobs when new.payload like '%"data":"database.recover-tail"%' begin select raise(abort, 'chain handoff rejected'); end"#
    };
    db.unprepared(reject).await.unwrap();
    let once = ["queue:work", "database", "--queue=batched-chains", "--once"];
    artisan_status(base, &once, 1).await;
    assert_eq!(repository.find(id).await.unwrap().unwrap().pending_jobs, 1);
    let allow = if mysql {
        "alter table jobs drop check reject_batched_tail"
    } else {
        "drop trigger reject_batched_tail"
    };
    db.unprepared(allow).await.unwrap();
    db.statement(
        "update jobs set reserved_at = 0 where queue = ?",
        &["batched-chains".into()],
    )
    .await
    .unwrap();
    artisan(base, &once).await;
    assert_eq!(chain_receipt(db, "database.recover-head").await, 1);
    assert_eq!(chain_receipt(db, "database.recover-tail").await, 0);
    artisan(base, &once).await;
    assert_eq!(repository.find(id).await.unwrap().unwrap().progress(), 100);
    assert_eq!(chain_receipt(db, "database.recover-tail").await, 1);
}

async fn chained_batch_id(db: &Connection, name: &str) -> String {
    let batch = db
        .table("job_batches")
        .r#where("name", "=", name)
        .get()
        .await
        .unwrap();
    assert_eq!(batch.len(), 1);
    match batch[0].get("id") {
        Some(Value::Text(id)) => id.clone(),
        _ => panic!("missing chained batch id"),
    }
}

async fn verify_chained_batches(
    base: &Path,
    db: &Connection,
    manager: &QueueManager,
    address: std::net::SocketAddr,
    connection: &str,
    success: &str,
) {
    let outer = manager.connection(Some(connection)).unwrap();
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    let outer_once = ["queue:work", connection, "--queue=chain", "--once"];
    for kind in [success, "allow", "deny", "cancel"] {
        let name = format!("chained-{connection}-{kind}");
        let path = format!("chained-batch/{connection}/{kind}");
        request(address, &path, 200).await;
        artisan(base, &outer_once).await;
        assert_eq!(
            db.table("job_batches")
                .r#where("name", "=", name.as_str())
                .count("*")
                .await
                .unwrap(),
            0
        );
        if connection == "database" && kind == "ok" {
            // Replay the same orchestration step after its batch is already persisted.
            db.statement(
                "insert into jobs (queue, payload, attempts, available_at, created_at) select queue, payload, 0, available_at, created_at from jobs where queue = ?",
                &["chain".into()],
            ).await.unwrap();
        }
        artisan(base, &outer_once).await;
        if connection == "database" && kind == "ok" {
            artisan(base, &outer_once).await;
        }
        let id = chained_batch_id(db, &name).await;
        assert_eq!(outer.size(Some("chain")).await.unwrap(), 0);
        assert_eq!(repository.find(&id).await.unwrap().unwrap().pending_jobs, 2);
        let inner = if kind == "cross" {
            "database"
        } else {
            connection
        };
        let inner_once = ["queue:work", inner, "--queue=inner-batch", "--once"];
        if kind == "cancel" {
            request(address, &format!("cancel-chained/{id}"), 200).await;
        }
        artisan(base, &inner_once).await;
        assert_eq!(outer.size(Some("chain")).await.unwrap(), 0);
        artisan(base, &inner_once).await;
        let batch = repository.find(&id).await.unwrap().unwrap();
        let continues = kind == success || kind == "allow";
        assert_eq!(
            outer.size(Some("chain")).await.unwrap(),
            u64::from(continues)
        );
        assert_eq!(batch.cancelled(), !continues);
        if continues {
            assert_eq!(callback_trace(db, &id).await, [("finally".into(), None)]);
            artisan(base, &outer_once).await;
        }
        assert_eq!(
            chain_receipt(db, &format!("{connection}.{kind}-last")).await,
            u64::from(continues)
        );
        assert_eq!(
            chain_receipt(db, &format!("{connection}.{kind}.fatal.catch")).await,
            u64::from(kind == "deny")
        );
    }
    request(address, &format!("chained-batch/{connection}/pair"), 200).await;
    artisan(base, &outer_once).await;
    let first = chained_batch_id(db, &format!("chained-{connection}-pair")).await;
    let inner = [
        "queue:work",
        connection,
        "--queue=inner-batch",
        "--stop-when-empty",
    ];
    artisan(base, &inner).await;
    assert!(repository.find(&first).await.unwrap().unwrap().finished());
    assert_eq!(
        chain_receipt(db, &format!("{connection}.pair-second")).await,
        0
    );
    artisan(base, &outer_once).await;
    let second = chained_batch_id(db, &format!("chained-{connection}-pair-second")).await;
    assert_eq!(outer.size(Some("chain")).await.unwrap(), 0);
    artisan(base, &inner).await;
    assert!(repository.find(&second).await.unwrap().unwrap().finished());
    artisan(base, &outer_once).await;
    assert_eq!(
        chain_receipt(db, &format!("{connection}.pair-last")).await,
        1
    );
}

async fn verify_chained_batch_recovery(
    base: &Path,
    db: &Connection,
    address: std::net::SocketAddr,
    redis: Option<&RedisConnection>,
    mysql: bool,
) {
    let connection = if redis.is_some() { "redis" } else { "database" };
    let name = format!("chained-{connection}-recover");
    request(address, &format!("chained-batch/{connection}/recover"), 200).await;
    let outer_once = ["queue:work", connection, "--queue=chain", "--once"];
    artisan(base, &outer_once).await;
    artisan(base, &outer_once).await;
    let id = chained_batch_id(db, &name).await;
    let ready = redis.map(|redis| redis.key("queues:chain"));
    if let Some((redis, ready)) = redis.zip(ready.as_deref()) {
        redis
            .command("SET", &[ready.as_bytes(), b"wrong type"])
            .await
            .unwrap();
    } else {
        let reject = if mysql {
            r#"alter table jobs add constraint reject_continuation check (payload not like '%"data":"database.recover-last"%')"#
        } else {
            r#"create trigger reject_continuation before insert on jobs when new.payload like '%"data":"database.recover-last"%' begin select raise(abort, 'continuation rejected'); end"#
        };
        db.unprepared(reject).await.unwrap();
    }
    let inner = [
        "queue:work",
        connection,
        "--queue=inner-batch",
        "--stop-when-empty",
    ];
    artisan(base, &inner).await;
    assert_eq!(
        db.table("job_batches_continuations")
            .r#where("batch_id", "=", id.as_str())
            .r#where("ready", "=", true)
            .count("*")
            .await
            .unwrap(),
        1
    );
    if let Some((redis, ready)) = redis.zip(ready.as_deref()) {
        redis.command("DEL", &[ready.as_bytes()]).await.unwrap();
    } else {
        let allow = if mysql {
            "alter table jobs drop check reject_continuation"
        } else {
            "drop trigger reject_continuation"
        };
        db.unprepared(allow).await.unwrap();
    }
    let publish = ["queue:publish-batches"];
    let (first, second) = tokio::join!(artisan(base, &publish), artisan(base, &publish));
    assert_eq!(
        [first, second]
            .iter()
            .filter(|line| line.contains("1 chained batch continuations published."))
            .count(),
        1
    );
    artisan(base, &outer_once).await;
    assert_eq!(
        chain_receipt(db, &format!("{connection}.recover-last")).await,
        1
    );
}

async fn verify_chain_edits(
    base: &Path,
    db: &Connection,
    address: std::net::SocketAddr,
    connection: &str,
) {
    let work = [
        "queue:work",
        connection,
        "--queue=chain",
        "--once",
        "--tries=2",
    ];
    for kind in ["edit-retry", "edit-release"] {
        let before = format!("{connection}.{kind}-before");
        let after = format!("{connection}.{kind}-after");
        let last = format!("{connection}.last");
        let previous = chain_receipt(db, &last).await;
        request(address, &format!("chain/{connection}/{kind}"), 200).await;
        for _ in 0..3 {
            artisan(base, &work).await;
            assert_eq!(chain_receipt(db, &before).await, 0);
            assert_eq!(chain_receipt(db, &after).await, 0);
        }
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &before).await, 1);
        assert_eq!(chain_receipt(db, &last).await, previous);
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &last).await, previous + 1);
        assert_eq!(chain_receipt(db, &after).await, 0);
        artisan(base, &work).await;
        assert_eq!(chain_receipt(db, &after).await, 1);
        assert_eq!(chain_receipt(db, &before).await, 1);
        assert_eq!(chain_receipt(db, &format!("{connection}.{kind}")).await, 2);
    }
    request(address, &format!("chain/{connection}/edit-fail"), 200).await;
    for _ in 0..5 {
        artisan(base, &work).await;
    }
    assert_eq!(
        chain_receipt(db, &format!("{connection}.edit-fail-before")).await,
        0
    );
    assert_eq!(
        chain_receipt(db, &format!("{connection}.edit-fail-after")).await,
        0
    );
    assert_eq!(
        chain_receipt(db, &format!("{connection}.edit-fail.catch")).await,
        1
    );

    request(address, &format!("chain/{connection}/edit-batch"), 200).await;
    artisan(base, &work).await;
    artisan(base, &work).await;
    artisan(base, &work).await;
    let id = chained_batch_id(db, &format!("{connection}.edit-batch")).await;
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    assert_eq!(repository.find(&id).await.unwrap().unwrap().pending_jobs, 1);
    artisan(
        base,
        &["queue:work", connection, "--queue=inner-batch", "--once"],
    )
    .await;
    assert_eq!(
        chain_receipt(db, &format!("{connection}.edit-batch-report")).await,
        1
    );
    artisan(base, &work).await;
}

async fn verify_batched_chain_edit(
    base: &Path,
    db: &Connection,
    address: std::net::SocketAddr,
    connection: &str,
    database: Option<bool>,
) {
    let batch = request(address, &format!("batched-chains/{connection}/edit"), 200)
        .await
        .unwrap();
    let id = batch["id"].as_str().unwrap();
    let repository =
        openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    let once = ["queue:work", connection, "--queue=batched-chains", "--once"];
    if let Some(mysql) = database {
        let reject = if mysql {
            r#"alter table jobs add constraint reject_edited_chain check (payload not like '%"data":"database.edit-batched-added"%')"#
        } else {
            r#"create trigger reject_edited_chain before insert on jobs when new.payload like '%"data":"database.edit-batched-added"%' begin select raise(abort, 'handoff rejected'); end"#
        };
        db.unprepared(reject).await.unwrap();
        artisan_status(base, &once, 1).await;
        let snapshot = repository.find(id).await.unwrap().unwrap();
        assert_eq!((snapshot.total_jobs, snapshot.pending_jobs), (3, 2));
        let allow = if mysql {
            "alter table jobs drop check reject_edited_chain"
        } else {
            "drop trigger reject_edited_chain"
        };
        db.unprepared(allow).await.unwrap();
        db.statement(
            "update jobs set reserved_at = 0 where queue = ?",
            &["batched-chains".into()],
        )
        .await
        .unwrap();
    }
    artisan(base, &once).await;
    let snapshot = repository.find(id).await.unwrap().unwrap();
    assert_eq!((snapshot.total_jobs, snapshot.pending_jobs), (3, 2));
    assert_eq!(
        chain_receipt(db, &format!("{connection}.edit-batched")).await,
        1
    );
    artisan(base, &once).await;
    assert_eq!(
        chain_receipt(db, &format!("{connection}.edit-batched-added")).await,
        1
    );
    artisan(base, &once).await;
    assert_eq!(repository.find(id).await.unwrap().unwrap().progress(), 100);
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
