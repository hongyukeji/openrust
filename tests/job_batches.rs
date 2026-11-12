use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        queue::JobHandler,
    },
    database::{DB, DatabaseManager},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{
        JobContext, Payload, QueueError, QueueFuture, QueueResult,
        middleware::{JobMiddleware, SkipIfBatchCancelled},
    },
    routing::{Router, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Bus, Queue},
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
            Ok(())
        })
    }
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
        let jobs = |names: &[&str]| {
            names
                .iter()
                .map(|name| Payload::new("reports.generate", name))
                .collect::<QueueResult<Vec<_>>>()
        };
        let batch = match kind {
            "success" => Bus::batch(jobs(&["summary", "detail"])?)
                .name("monthly reports")
                .onQueue("batch"),
            "allow" => Bus::batch(jobs(&["failed", "after"])?)
                .name("allow")
                .onQueue("batch")
                .allowFailures(),
            "cancel" => Bus::batch(jobs(&["failed", "skipped"])?)
                .name("cancel")
                .onQueue("batch"),
            "rollback" => Bus::batch(jobs(&["first", "second"])?).name("rollback"),
            "sync" => Bus::batch(jobs(&["skipped"])?).onConnection("sync"),
            _ => unreachable!(),
        }
        .dispatch()
        .await?;
        Ok(Response::json(StatusCode::OK, &batch).unwrap())
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/batch/{kind}", route)?;
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
    assert_eq!(application(&base).handleCommand(&args).await.unwrap(), 0);
}

async fn artisan(base: &Path, args: &[&str]) {
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
            .env("OPENRUST_BATCH_ARGS", serde_json::to_string(args).unwrap())
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
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
    let settings = json!({
        "database": {"default":"batch", "connections":{"batch":connection}},
        "queue": {
            "default":"database",
            "connections": {"database":{"driver":"database","retry_after":90}},
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
    ] {
        schema.dropIfExists(table).await.unwrap();
    }
    artisan(&root.0, &["migrate", "--force"]).await;
    db.statement("create table receipts (name varchar(255) not null)", &[])
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
    let batches = openrust::bus::DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    request(address, "sync", 500).await;
    assert_eq!(db.table("job_batches").count("*").await.unwrap(), 0);
    let success = request(address, "success", 200).await.unwrap();
    let id = success["id"].as_str().unwrap();
    assert_eq!(success["pendingJobs"], 2);
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
        if kind == "allow" {
            db.statement("insert into receipts (name) values ('allow')", &[])
                .await
                .unwrap();
            artisan(&root.0, &["queue:retry", &batch.failed_job_ids[0]]).await;
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
    server.abort();
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
