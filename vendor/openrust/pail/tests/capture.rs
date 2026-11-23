use openrust::{
    auth::{AuthFuture, AuthIdentifier, AuthManager},
    config::{ConfigError, Environment, Value},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{
        auth::Authenticatable, container::Container as _, foundation::Application as _,
        queue::JobHandler,
    },
    database::DatabaseManager,
    foundation::{Application, configuration::Routing, http::Kernel, run_application},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::{Level, LogContext},
    queue::{
        DispatchOptions, JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueManager,
        QueueResult,
    },
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Auth, Bus, Context, Log, Queue, Route},
    },
};
use openrust_pail::{
    File, Options, PailServiceProvider, contracts::Printer, printers::CliPrinter,
    value_objects::MessageLogged,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

type SetupError = Box<dyn Error + Send + Sync>;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("openrust-pail-capture-{:032x}", fastrand::u128(..)));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join(".env"), "PAIL_TESTS=true\n").unwrap();
        for name in ["one", "two"] {
            File::new(root.join(format!("storage/pail/{name}.pail")))
                .create()
                .unwrap();
        }
        Self(root)
    }
    fn records(&self, name: &str) -> Vec<serde_json::Value> {
        let directory = std::fs::read_to_string(self.0.join("storage-override"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.0.join("storage"));
        std::fs::read_to_string(directory.join(format!("pail/{name}.pail")))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing","host":"127.0.0.1","port":0}),
        ),
        (
            "logging".into(),
            json!({"default":"null","channels": {"null":{"driver":"null"},"single":{"driver":"single","path":"written.log"}}}),
        ),
        (
            "auth".into(),
            json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"capture"},"other":{"driver":"capture-other"}}}),
        ),
        (
            "database".into(),
            json!({"default":"mysql","connections":{"mysql":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap_or_default()}}}),
        ),
        (
            "queue".into(),
            json!({"default":"database","connections":{"database":{"driver":"database","table":"jobs","queue":"pail","retry_after":5},"sync":{"driver":"sync"}},"failed":{"driver":"database-uuids","database":"mysql","table":"failed_jobs"}}),
        ),
    ]))
}

struct User(String);
impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::String(self.0.clone())
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(
        &self,
        name: &str,
    ) -> openrust::auth::AuthResult<Option<openrust::database::Value>> {
        Ok((name == "email")
            .then(|| openrust::database::Value::Text(format!("{}@example.test", self.0))))
    }
}

struct Probe;
impl ServiceProvider for Probe {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let auth = app.make::<AuthManager>().await?;
            auth.via_request("capture", |request| -> AuthFuture<'_, _> {
                let id = request.route("id").unwrap().to_owned();
                Box::pin(async move { Ok(Some(Box::new(User(id)) as Box<dyn Authenticatable>)) })
            });
            auth.via_request("capture-other", |_| -> AuthFuture<'_, _> {
                Box::pin(async {
                    Ok(Some(
                        Box::new(User("other".into())) as Box<dyn Authenticatable>
                    ))
                })
            });
            for name in ["capture-probe", "capture-left", "capture-right"] {
                Queue::register(name, Arc::new(Job))?;
            }
            Ok(())
        })
    }
}

struct Job;
impl JobHandler for Job {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            Log::info(
                "job captured",
                &LogContext::from_iter([("expected_job".into(), json!(payload.name()))]),
            )
            .unwrap();
            if payload.options().hard_timeout {
                let _ = openrust::http::client::Factory::new()
                    .unwrap()
                    .batch(|batch| {
                        batch.finally(|_, _| {
                            Log::info("isolated deferred captured", &LogContext::new()).unwrap();
                        });
                    })
                    .defer()
                    .unwrap();
            }
            if payload.data() == "fail" {
                return Err(QueueError::Execution {
                    source: std::sync::Arc::new(std::io::Error::other("capture failed")),
                });
            }
            Ok(())
        })
    }
    fn failed<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        _: &'a Payload,
        _: &'a QueueError,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async {
            Log::warning("failed callback captured", &LogContext::new()).unwrap();
            Ok(())
        })
    }
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/capture/{id}", capture_http)
}
fn capture_http(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Context::add("scope", "public".into()).unwrap();
        Context::add_hidden("secret", "hidden".into()).unwrap();
        Log::info("before identity", &LogContext::new()).unwrap();
        Auth::user(request).await.unwrap();
        tokio::task::yield_now().await;
        Log::info("after identity", &LogContext::new()).unwrap();
        Auth::manager()
            .unwrap()
            .should_use(request, "other")
            .unwrap();
        Auth::user(request).await.unwrap();
        Log::info("other identity", &LogContext::new()).unwrap();
        Artisan::call_silently("capture:log", &[]).await.unwrap();
        Log::info("back to HTTP", &LogContext::new()).unwrap();
        Ok(Response::new(StatusCode::OK, Body::from("captured")))
    })
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command("capture:log", |output| {
        Log::info("command captured", &LogContext::new()).unwrap();
        Log::warning("Deprecated hidden", &LogContext::new()).unwrap();
        Log::error("Deprecated error visible", &LogContext::new()).unwrap();
        Log::log_exception(
            Level::Error,
            "exception captured",
            &std::io::Error::other("exception details"),
            &LogContext::new(),
        )
        .unwrap();
        output.comment("captured");
    })?
    .purpose("Verify Pail session capture")
}
struct ConcurrentCommand(CommandDefinition);
impl Command for ConcurrentCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let name = self.0.name();
            if name == "capture:parallel" {
                let (left, right) = tokio::join!(
                    Artisan::call_silently("capture:left", &[]),
                    Artisan::call_silently("capture:right", &[])
                );
                assert_eq!(left?, 0);
                assert_eq!(right?, 0);
            } else if name == "capture:parallel-jobs" {
                let (left, right) = tokio::join!(
                    Bus::dispatch_sync(
                        Payload::new("capture-left", "success").unwrap(),
                        DispatchOptions::default(),
                        None
                    ),
                    Bus::dispatch_sync(
                        Payload::new("capture-right", "success").unwrap(),
                        DispatchOptions::default(),
                        None
                    )
                );
                left.unwrap();
                right.unwrap();
            } else {
                tokio::time::sleep(Duration::from_millis(if name == "capture:left" {
                    20
                } else {
                    1
                }))
                .await;
            }
            Log::info(name, &LogContext::new()).unwrap();
            Ok(0)
        })
    }
}
struct Inspect(CommandDefinition);
impl Command for Inspect {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let options = Options::from_command(input)?;
            let contents =
                std::fs::read_to_string(app.base_path().join("storage/pail/one.pail")).unwrap();
            let selected: Vec<_> = contents
                .lines()
                .map(|line| MessageLogged::from_json(line).unwrap())
                .filter(|message| options.accepts(message))
                .map(|message| message.to_string())
                .collect();
            let result = json!({"timeout":options.timeout(),"selected":selected});
            std::fs::write(app.base_path().join("inspection.json"), result.to_string()).unwrap();
            if input.matches().get_flag("print") {
                let mut printer = CliPrinter::new(output, app.base_path());
                for record in &selected {
                    printer.print(&MessageLogged::from_json(record).unwrap())?;
                }
            } else {
                output
                    .stdout
                    .push_str(&format!("{} selected\n", selected.len()));
            }
            Ok(0)
        })
    }
}
fn async_commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    let mut inspect = clap::Command::new("capture:inspect").arg(
        clap::Arg::new("print")
            .long("print")
            .action(clap::ArgAction::SetTrue),
    );
    for name in ["auth", "user", "level", "filter", "message", "timeout"] {
        let mut arg = clap::Arg::new(name).long(name).num_args(1);
        if name == "timeout" {
            arg = arg.default_value("3600");
        }
        inspect = inspect.arg(arg);
    }
    artisan.add_command(Box::new(Inspect(CommandDefinition::new(inspect))))?;
    for name in [
        "capture:parallel",
        "capture:left",
        "capture:right",
        "capture:parallel-jobs",
    ] {
        artisan.add_command(Box::new(ConcurrentCommand(CommandDefinition::new(
            clap::Command::new(name),
        ))))?;
    }
    Ok(())
}

fn application(root: PathBuf) -> Result<Application, SetupError> {
    let storage = std::fs::read_to_string(root.join("storage-override")).ok();
    let app = Application::configure(root)
        .with_config(config)
        .with_commands(async_commands)
        .with_providers(vec![Box::new(PailServiceProvider), Box::new(Probe)])
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()?;
    if let Some(storage) = storage {
        app.use_storage_path(storage)?;
    }
    Ok(app)
}

async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(application(root.0.clone()).unwrap());
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let service = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handle_transport(request));
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}
async fn http(address: std::net::SocketAddr, id: &str) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(
            format!(
                "GET /capture/{id}?q=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    assert!(
        String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
}
async fn artisan(root: &Root, args: &[&str]) {
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &root.0)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result.status.success(), "{result:?}");
}

async fn run_acceptance() {
    let left = Root::new();
    let right = Root::new();
    let (left_address, left_server) = server(&left).await;
    let (right_address, right_server) = server(&right).await;
    tokio::join!(http(left_address, "left"), http(right_address, "right"));
    for (root, id) in [(&left, "left"), (&right, "right")] {
        let records = root.records("one");
        assert_eq!(records.len(), 7);
        let first = &records[0]["context"];
        assert_eq!(
            first["__pail"]["origin"]["auth_id"],
            serde_json::Value::Null
        );
        assert_eq!(first["scope"], "public");
        assert!(first.get("secret").is_none());
        let identity = &records[1]["context"]["__pail"]["origin"];
        assert_eq!(identity["type"], "http");
        assert_eq!(identity["method"], "GET");
        assert_eq!(identity["path"], format!("capture/{id}"));
        assert_eq!(identity["auth_id"], id);
        assert_eq!(identity["auth_email"], format!("{id}@example.test"));
        assert_eq!(
            records[2]["context"]["__pail"]["origin"]["auth_id"],
            "other"
        );
        assert!(
            records
                .iter()
                .all(|record| record["context"]["__pail"]["origin"]["type"] == "http")
        );
        assert_eq!(
            records.last().unwrap()["context"]["__pail"]["origin"]["auth_id"],
            "other"
        );
        assert!(
            records
                .iter()
                .any(|record| record["context"].get("exception").is_some())
        );
        assert_eq!(root.records("two").len(), records.len());
    }
    for (root, id) in [(&left, "left"), (&right, "right")] {
        artisan(
            root,
            &[
                "capture:inspect",
                &format!("--auth={id}"),
                "--level=iNfO",
                "--message=after IDENTITY",
                "--filter=get",
                "--timeout=1e2",
            ],
        )
        .await;
        let result: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.0.join("inspection.json")).unwrap())
                .unwrap();
        assert_eq!(result["timeout"], 100);
        assert_eq!(result["selected"].as_array().unwrap().len(), 1);
        let message = MessageLogged::from_json(result["selected"][0].as_str().unwrap()).unwrap();
        assert_eq!(message.auth_id().as_deref(), Some(id));
        assert_eq!(message.message(), "after identity");
    }
    for (root, id) in [(&left, "left"), (&right, "right")] {
        for flags in [
            vec![],
            vec!["-v"],
            vec!["-vv"],
            vec!["--ansi"],
            vec!["--quiet"],
            vec!["--silent"],
        ] {
            let result = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "artisan",
                    "capture:inspect",
                    "--print",
                    &format!("--auth={id}"),
                    "--message=after identity",
                ])
                .args(&flags)
                .env("APP_BASE_PATH", &root.0)
                .env("COLUMNS", "50")
                .output()
                .await
                .unwrap();
            assert!(result.status.success(), "{result:?}");
            let text = String::from_utf8(result.stdout).unwrap();
            if flags
                .iter()
                .any(|flag| matches!(*flag, "--quiet" | "--silent"))
            {
                assert!(text.is_empty());
            } else {
                assert!(text.contains("after identity"));
                assert!(text.contains(&format!("GET: /capture/{id}")));
                assert!(text.contains(&format!("Auth ID: {id} ({id}@example.test)")));
                assert_eq!(text.contains("\x1b["), flags.contains(&"--ansi"));
                assert!(text.lines().next().unwrap().contains("INFO"));
            }
        }
    }
    println!(
        "Pail real HTTP to independent Artisan printer: both applications, verbosity, ANSI and quiet/silent passed"
    );
    println!("Pail actual HTTP capture and independent Artisan option filtering passed");
    artisan(&left, &["capture:log"]).await;
    let records = left.records("one");
    assert_eq!(records.len(), 10);
    for record in &records[7..] {
        assert_eq!(record["context"]["__pail"]["origin"]["type"], "console");
        assert_eq!(
            record["context"]["__pail"]["origin"]["command"],
            "capture:log"
        );
    }
    assert_eq!(right.records("one").len(), 7);
    File::new(left.0.join("storage/pail/one.pail"))
        .destroy()
        .unwrap();
    artisan(&left, &["capture:log"]).await;
    assert!(!left.0.join("storage/pail/one.pail").exists());
    assert_eq!(left.records("two").len(), 13);
    artisan(&right, &["capture:parallel"]).await;
    let concurrent = right.records("one");
    assert_eq!(concurrent.len(), 10);
    for record in &concurrent[7..] {
        assert_eq!(
            record["context"]["__pail"]["origin"]["command"],
            record["message"]
        );
    }
    artisan(&right, &["capture:parallel-jobs"]).await;
    let jobs = right.records("one");
    assert_eq!(jobs.len(), 13);
    for record in &jobs[10..12] {
        assert_eq!(record["message"], "job captured");
        assert_eq!(
            record["context"]["__pail"]["origin"]["job"],
            record["context"]["expected_job"]
        );
        assert_eq!(
            record["context"]["__pail"]["origin"]["command"],
            "capture:parallel-jobs"
        );
    }
    assert_eq!(jobs[12]["context"]["__pail"]["origin"]["type"], "console");
    println!(
        "Pail real HTTP, independent Artisan, concurrent jobs, identity and session isolation passed"
    );
    left_server.abort();
    right_server.abort();
    let _ = left_server.await;
    let _ = right_server.await;
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_some() {
        worker_acceptance(&right).await;
        let custom = Root::new();
        let directory = custom.0.join("custom-worker-storage");
        std::fs::write(
            custom.0.join("storage-override"),
            directory.to_string_lossy().as_bytes(),
        )
        .unwrap();
        for name in ["one", "two"] {
            File::new(directory.join(format!("pail/{name}.pail")))
                .create()
                .unwrap();
        }
        worker_acceptance(&custom).await;
        assert!(
            std::fs::read_to_string(custom.0.join("storage/pail/one.pail"))
                .unwrap()
                .is_empty()
        );
        println!("Pail MySQL Worker and isolated callbacks honor custom application storage path");
    } else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL URL required"
        );
        eprintln!("Pail MySQL Worker acceptance skipped because no URL was provided");
    }
}

async fn worker_acceptance(root: &Root) {
    let mut app = application(root.0.clone()).unwrap();
    app.boot().await.unwrap();
    let database = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let mut schema = database.get_schema_builder();
    schema.drop_if_exists("jobs").await.unwrap();
    schema.drop_if_exists("failed_jobs").await.unwrap();
    schema
        .create("jobs", |table| {
            table.id();
            table.string("queue", 255).index();
            table.long_text("payload");
            table.unsigned_integer("attempts");
            table.unsigned_big_integer("reserved_at").nullable();
            table.unsigned_big_integer("available_at");
            table.unsigned_big_integer("created_at");
        })
        .await
        .unwrap();
    schema
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", 255).unique();
            table.text("connection");
            table.text("queue");
            table.long_text("payload");
            table.long_text("exception");
            table.timestamp("failed_at", None).use_current();
        })
        .await
        .unwrap();
    let manager = app.container().make::<QueueManager>().unwrap();
    let before = root.records("one").len();
    for (kind, isolated) in [("fail", false), ("success", false), ("success", true)] {
        let payload = Payload::new("capture-probe", kind)
            .unwrap()
            .with_options(JobOptions {
                hard_timeout: isolated,
                max_tries: Some(1),
                ..Default::default()
            });
        manager
            .dispatch(payload, DispatchOptions::default(), None)
            .await
            .unwrap();
    }
    artisan(
        root,
        &[
            "queue:work",
            "--queue=pail",
            "--stop-when-empty",
            "--sleep=0",
            "--tries=1",
            "--timeout=5",
        ],
    )
    .await;
    let records = root.records("one");
    let produced = &records[before..];
    assert!(
        produced
            .iter()
            .filter(|record| record["message"] == "job captured")
            .count()
            == 3
    );
    assert_eq!(
        produced
            .iter()
            .filter(|record| record["message"] == "failed callback captured")
            .count(),
        1
    );
    assert_eq!(
        produced
            .iter()
            .filter(|record| record["message"] == "isolated deferred captured")
            .count(),
        1
    );
    for record in produced.iter().filter(|record| {
        [
            "job captured",
            "failed callback captured",
            "isolated deferred captured",
        ]
        .iter()
        .any(|message| record["message"] == *message)
    }) {
        let origin = &record["context"]["__pail"]["origin"];
        assert_eq!(origin["type"], "queue");
        assert_eq!(origin["command"], "queue:work");
        assert_eq!(origin["queue"], "pail");
        assert_eq!(origin["job"], "capture-probe");
    }
    assert_eq!(database.table("failed_jobs").count("*").await.unwrap(), 1);
    assert_eq!(
        manager.connection(None).unwrap().size(None).await.unwrap(),
        0
    );
    schema.drop_if_exists("jobs").await.unwrap();
    schema.drop_if_exists("failed_jobs").await.unwrap();
    println!(
        "Pail real MySQL Worker, isolated job, failed callback and following consumption passed"
    );
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("artisan") {
        return run_application(|| {
            application(PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap()))
        })
        .await;
    }
    run_acceptance().await;
    ExitCode::SUCCESS
}
