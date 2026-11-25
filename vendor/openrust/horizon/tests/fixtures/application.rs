//! Dedicated process acceptance application; never registered in the default app.
mod command_handlers;

use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::events::{Dispatcher as _, Event, Listener},
    contracts::foundation::Application as _,
    contracts::queue::JobHandler,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request},
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueManager, QueueResult},
    support::{ProviderResult, ServiceProvider},
};
use openrust_horizon::{
    AddSupervisor, Horizon, HorizonServiceProvider, MasterSupervisor, MasterSupervisorDeployed,
    MasterSupervisorLooped, MasterSupervisorOutOfMemory, ProvisioningPlan,
    RedisHorizonCommandQueue, Supervisor, SupervisorLooped, SupervisorOptions,
    SupervisorOutOfMemory, UnableToLaunchProcess, WorkerProcessRestarting,
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
fn config(base: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
    let defaults = if std::env::var("HORIZON_PROBE_EMPTY_DEFAULTS").as_deref() == Ok("1") {
        json!({})
    } else {
        let timeout = std::env::var("HORIZON_PROBE_WORKER_TIMEOUT")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(5);
        json!({"supervisor-1":{"timeout":timeout,"sleep":1,"balanceCooldown":1,"memory":1024,"maxProcesses":2,"maxJobs":3,"workersName":"horizon-probe","tries":2,"backoff":[1,2],"rest":1,"force":true,"json":true,"nice":i32::from(cfg!(unix))}})
    };
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing","debug":false})),
        (
            "database".into(),
            json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":base.join("database.sqlite")}},"redis":{"options":{"prefix":format!("{}:",base.file_name().unwrap().to_str().unwrap())},"default":{"url":std::env::var("HORIZON_TEST_REDIS_URL").unwrap()}}}),
        ),
        (
            "cache".into(),
            json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache")}}}),
        ),
        (
            "queue".into(),
            json!({"default":"redis","connections":{"redis":{"driver":"redis","connection":"default","queue":"default","retry_after":30,
                "block_for": std::env::var("HORIZON_PROBE_BLOCK_FOR").ok().map(|value| value.parse::<u32>().unwrap())
            }},"failed":{"driver":"database-uuids","database":"sqlite","table":"failed_jobs"}}),
        ),
        (
            "horizon".into(),
            json!({
                "retention": 3600,
                "fast_termination": std::env::var("HORIZON_PROBE_FAST_TERMINATION").as_deref() == Ok("1"),
                "memory_limit": std::env::var("HORIZON_PROBE_MEMORY_LIMIT")
                    .ok().and_then(|value| value.parse::<u64>().ok()).unwrap_or(64),
                "defaults": defaults,
                "environments": {
                    "testing": {},
                    "empty": {},
                    "disabled": {"supervisor-1": {"maxProcesses": 0}},
                    "cooldown": {"supervisor-1": {"maxProcesses": 1}},
                    "spawn": {"supervisor-1": {"maxProcesses": 1, "directory": base.join("worker-cwd")}},
                    "queue-names": {"supervisor-1": {"maxProcesses": 1, "balance": false, "queue": "出库 汇总:报表,  打包复核"}},
                    "public-plan*": {"planned": {"timeout": 5, "maxProcesses": 1, "queue": ["planned"]}, "disabled-plan": {"maxProcesses": 0}},
                    "public-plan": {"excluded": {"timeout": 5, "maxProcesses": 1, "queue": ["excluded"]}},
                },
            }),
        ),
    ]))
}
struct Probe(std::path::PathBuf);
impl JobHandler for Probe {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let data = payload.data();
            if data.get("fail").and_then(Value::as_bool) == Some(true) {
                return Err(QueueError::InvalidPayload);
            }
            use std::io::Write;
            if let Some(marker) = data.get("probe_output").and_then(Value::as_str) {
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "worker-out-東京-{marker}").and_then(|()| stdout.flush());
                let mut stderr = std::io::stderr().lock();
                let _ = write!(stderr, "worker-err-{marker}").and_then(|()| stderr.flush());
            }
            if let Some(marker) = data.get("probe_format").and_then(Value::as_str) {
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(
                    stdout,
                    "<info>styled-{marker}</info> <unknown>{marker}</unknown>"
                )
                .and_then(|()| stdout.flush());
            }
            if data.get("probe_split_format").and_then(Value::as_bool) == Some(true) {
                {
                    let mut stdout = std::io::stdout().lock();
                    let _ = write!(stdout, "<info>styled").and_then(|()| stdout.flush());
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "-split</info>").and_then(|()| stdout.flush());
            }
            if data.get("record_start").and_then(Value::as_bool) == Some(true) {
                writeln!(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(self.0.with_file_name("started.log"))
                        .unwrap(),
                    "{}",
                    payload.uuid()
                )
                .unwrap();
            }
            tokio::time::sleep(Duration::from_millis(
                data.get("sleep_ms").and_then(Value::as_u64).unwrap_or(20),
            ))
            .await;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.0)
                    .unwrap(),
                "{}",
                payload.uuid()
            )
            .unwrap();
            Ok(())
        })
    }
}
struct ProbeProvider;

struct MemoryEventRecorder(std::path::PathBuf);

struct ProcessEventRecorder(
    std::path::PathBuf,
    Option<(Arc<openrust::redis::connections::Connection>, String)>,
);

impl Listener for ProcessEventRecorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            use std::io::Write as _;
            let (file, row) = if let Some(restart) = event.downcast_ref::<WorkerProcessRestarting>()
            {
                (
                    "restart-events.jsonl",
                    json!({
                        "name": event.name(),
                        "supervisor": restart.supervisor,
                        "queue": restart.queue,
                        "pid": restart.pid,
                        "exit_code": restart.exit_code,
                    }),
                )
            } else if let Some(failure) = event.downcast_ref::<UnableToLaunchProcess>() {
                (
                    "restart-events.jsonl",
                    json!({
                        "name": event.name(),
                        "supervisor": failure.supervisor,
                        "queue": failure.queue,
                        "pid": failure.pid,
                        "exit_code": failure.exit_code,
                    }),
                )
            } else if let Some(deployment) = event.downcast_ref::<MasterSupervisorDeployed>() {
                if let Some((redis, prefix)) = &self.1 {
                    let key = redis.key(&format!(
                        "{prefix}{{horizon}}:commands:queued:master:{}",
                        deployment.master
                    ));
                    assert_eq!(
                        redis.command("LLEN", &[key.as_bytes()]).await.unwrap(),
                        openrust::redis::Value::Int(deployment.supervisors.len() as i64)
                    );
                }
                (
                    "deployment-events.jsonl",
                    json!({
                        "name": event.name(),
                        "master": deployment.master,
                        "environment": deployment.environment,
                        "supervisors": deployment.supervisors,
                    }),
                )
            } else {
                return Ok(None);
            };
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.0.join(file))
                    .unwrap(),
                "{row}"
            )
            .unwrap();
            if event.name() == MasterSupervisorDeployed::NAME
                && std::env::var_os("HORIZON_PROBE_DEPLOY_PLAN").is_some()
            {
                Ok(None)
            } else {
                Err(EventError::InvalidPayload)
            }
        })
    }
}

struct LoopDispatchFailure(std::path::PathBuf);

impl Listener for LoopDispatchFailure {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            use std::io::Write as _;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.0)
                    .unwrap(),
                "{}",
                event.name()
            )
            .unwrap();
            Err(EventError::InvalidPayload)
        })
    }
}

impl Listener for MemoryEventRecorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let (name, usage, limit) =
                if let Some(event) = event.downcast_ref::<MasterSupervisorOutOfMemory>() {
                    (&event.name, event.memory_usage, event.memory_limit)
                } else if let Some(event) = event.downcast_ref::<SupervisorOutOfMemory>() {
                    (&event.name, event.get_memory_usage(), event.memory_limit)
                } else {
                    return Ok(None);
                };
            use std::io::Write as _;
            let row = json!({"event":event.name(),"name":name,"usage":usage,"limit":limit});
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.0)
                    .unwrap(),
                "{row}"
            )
            .unwrap();
            // A failing application listener must not cancel the memory-limit exit.
            Err(EventError::InvalidPayload)
        })
    }
}

impl ServiceProvider for ProbeProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if std::env::var("HORIZON_PROBE_COMMAND_HANDLERS").as_deref() == Ok("1") {
                command_handlers::register(app)?;
            }
            if let Ok(basename) = std::env::var("HORIZON_PROBE_MASTER_BASENAME") {
                MasterSupervisor::determine_name_using(move || basename.clone())?;
            }
            if std::env::var_os("HORIZON_PROBE_PRELOAD_MASTER_COMMAND").is_some()
                && std::env::args().nth(2).as_deref() == Some("horizon")
            {
                let queue = app.make::<RedisHorizonCommandQueue>().await?;
                let name = MasterSupervisor::process_name()?;
                queue
                    .push(
                        &MasterSupervisor::command_queue()?,
                        AddSupervisor::NAME,
                        json!({"name": format!("{name}:stale")}),
                    )
                    .await?;
            }
            app.make::<QueueManager>().await?.register(
                "probe",
                Arc::new(Probe(app.base_path().join("completed.log"))),
            )?;
            let dispatcher = app.make::<openrust::events::Dispatcher>().await?;
            let recorder = Arc::new(MemoryEventRecorder(
                app.base_path().join("memory-events.jsonl"),
            ));
            dispatcher.listen(
                MasterSupervisorOutOfMemory::NAME,
                recorder.clone(),
                Delivery::Sync,
            )?;
            dispatcher.listen(SupervisorOutOfMemory::NAME, recorder, Delivery::Sync)?;
            let probe = if std::env::var_os("HORIZON_PROBE_ASSERT_QUEUED_STARTUP").is_some() {
                let redis = app
                    .make::<openrust::redis::RedisManager>()
                    .await?
                    .connection(None)?;
                let prefix = app
                    .config()?
                    .get("horizon.prefix")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned();
                Some((redis, prefix))
            } else {
                None
            };
            let recorder = Arc::new(ProcessEventRecorder(app.base_path().to_owned(), probe));
            dispatcher.listen(
                WorkerProcessRestarting::NAME,
                recorder.clone(),
                Delivery::Sync,
            )?;
            dispatcher.listen(
                UnableToLaunchProcess::NAME,
                recorder.clone(),
                Delivery::Sync,
            )?;
            dispatcher.listen(MasterSupervisorDeployed::NAME, recorder, Delivery::Sync)?;
            if std::env::var("HORIZON_PROBE_FAIL_LOOP_DISPATCH").as_deref() == Ok("1") {
                let listener =
                    Arc::new(LoopDispatchFailure(app.base_path().join("loop-errors.log")));
                dispatcher.listen(
                    MasterSupervisorLooped::NAME,
                    listener.clone(),
                    Delivery::Sync,
                )?;
                dispatcher.listen(SupervisorLooped::NAME, listener, Delivery::Sync)?;
            }
            Ok(())
        })
    }
}
fn authorize(request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    let allowed = request
        .headers()
        .get("x-test-operator")
        .is_some_and(|v| v == "operator");
    Box::pin(async move { Ok(allowed) })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::auth(authorize).unwrap();
    Horizon::routes()
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    let base = std::env::var("APP_BASE_PATH")?;
    let mut providers: Vec<Box<dyn ServiceProvider>> =
        vec![Box::new(HorizonServiceProvider), Box::new(ProbeProvider)];
    if std::env::var("HORIZON_PROBE_FAIL_LOOP_DISPATCH").as_deref() == Ok("1") {
        providers.swap(0, 1);
    }
    Ok(Application::configure(base.into())
        .with_config(config)
        .with_providers(providers)
        .with_routing(Routing::default().web(routes))
        .create()?)
}

async fn run_output_probe(role: &str) {
    use openrust::{
        process::OutputStream,
        support::facades::{App, Facade as _},
    };
    use std::io::Write as _;

    let mut app = create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let repository = app.make::<openrust_horizon::Repository>().await.unwrap();
        let queues = app.make::<QueueManager>().await.unwrap();
        let executable = std::env::current_exe().unwrap();
        let base = app.base_path().to_owned();
        let directory = base.clone();
        let receive = move |stream, bytes: &[u8]| {
            let file = match stream {
                OutputStream::Stdout => "callback.stdout",
                OutputStream::Stderr => "callback.stderr",
            };
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join(file))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        };
        let shutdown = tokio_util::sync::CancellationToken::new();
        #[cfg(unix)]
        let _monitor = {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                term.recv().await;
                shutdown.cancel();
            })
        };
        let options = BTreeMap::from([(
            "callback".to_string(),
            SupervisorOptions {
                name: "callback".to_string(),
                timeout: 5,
                sleep: 1,
                ..Default::default()
            },
        )]);
        match role {
            "master" => {
                let master = MasterSupervisor::new(
                    repository,
                    queues,
                    options,
                    executable,
                    base,
                    "testing".into(),
                )
                .unwrap()
                .handle_output_using(receive);
                master.output(OutputStream::Stderr, b"direct-master\n");
                master.run(&shutdown).await.unwrap();
            }
            "supervisor" => {
                let supervisor = Supervisor::new(
                    repository,
                    queues,
                    options,
                    executable,
                    base,
                    "testing".into(),
                )
                .unwrap()
                .handle_output_using(receive);
                supervisor.output(OutputStream::Stderr, b"direct-supervisor\n");
                supervisor.run(&shutdown).await.unwrap();
            }
            _ => panic!("unknown output probe: {role}"),
        }
    })
    .await;
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if let Ok(role) = std::env::var("HORIZON_PROBE_OUTPUT_CALLBACK")
        && std::env::args_os().len() == 1
    {
        run_output_probe(&role).await;
        return std::process::ExitCode::SUCCESS;
    }
    if let Ok(probe) = std::env::var("HORIZON_PROBE_DEPLOY_PLAN") {
        let mut app = create().unwrap();
        app.boot().await.unwrap();
        let args: Value = serde_json::from_str(&probe).unwrap();
        let plan = ProvisioningPlan::new(
            args["master"].as_str().unwrap(),
            app.config().unwrap().as_ref(),
        )
        .unwrap();
        plan.deploy(args["environment"].as_str().unwrap(), &app)
            .await
            .unwrap();
        return std::process::ExitCode::SUCCESS;
    }
    if std::env::var_os("HORIZON_PROBE_RECORD_SUPERVISOR_CWD").is_some()
        && std::env::args().nth(2).as_deref() == Some("horizon:supervisor")
    {
        let path = std::path::PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
        std::fs::write(
            path.join("supervisor-cwd.json"),
            json!(std::env::current_dir().unwrap()).to_string(),
        )
        .unwrap();
    }
    if let Ok(probe) = std::env::var("HORIZON_PROBE_COMMAND_QUEUE") {
        let mut app = create().unwrap();
        app.boot().await.unwrap();
        let queue = app.make::<RedisHorizonCommandQueue>().await.unwrap();
        let args: Value = serde_json::from_str(&probe).unwrap();
        let name = args["name"].as_str().unwrap();
        let name = if args["master"].as_bool() == Some(true) {
            MasterSupervisor::command_queue_for(Some(name)).unwrap()
        } else {
            name.to_owned()
        };
        match args["action"].as_str().unwrap() {
            "push" => {
                let command = args["command"].as_str().unwrap_or(AddSupervisor::NAME);
                queue
                    .push(&name, command, args["options"].clone())
                    .await
                    .unwrap();
            }
            "pending" => println!("{}", json!(queue.pending(&name).await.unwrap())),
            "flush" => queue.flush(&name).await.unwrap(),
            _ => panic!("unknown action"),
        }
        return std::process::ExitCode::SUCCESS;
    }
    if std::env::var("HORIZON_PROBE_PREPARE_DATABASE").as_deref() == Ok("1") {
        let mut app = create().unwrap();
        app.boot().await.unwrap();
        let manager = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        manager.connection(Some("sqlite")).unwrap().unprepared(
            "CREATE TABLE failed_jobs (id INTEGER PRIMARY KEY, uuid TEXT UNIQUE, connection TEXT, queue TEXT, payload TEXT, exception TEXT, failed_at TEXT)",
        ).await.unwrap();
        return std::process::ExitCode::SUCCESS;
    }
    // Controlled process-inspector fixture. It keeps the real executable and
    // Horizon argv visible after the launcher exits, without worker self-reaping.
    if std::env::var("HORIZON_PROBE_HOLD").as_deref() == Ok("1") {
        #[cfg(unix)]
        {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .unwrap()
                .recv()
                .await;
            return std::process::ExitCode::SUCCESS;
        }
        #[cfg(not(unix))]
        return std::process::ExitCode::FAILURE;
    }
    if let Ok(data) = std::env::var("HORIZON_PROBE_DISPATCH") {
        let mut app = create().unwrap();
        app.boot().await.unwrap();
        let manager = app.make::<QueueManager>().await.unwrap();
        let payload = Payload::new("probe", serde_json::from_str::<Value>(&data).unwrap()).unwrap();
        let queue = std::env::var("HORIZON_PROBE_DISPATCH_QUEUE").ok();
        let id = manager
            .connection(None)
            .unwrap()
            .push(&payload, queue.as_deref())
            .await
            .unwrap()
            .expect("Redis transport ID");
        println!(
            "{}",
            if std::env::var("HORIZON_PROBE_DISPATCH_UUID").as_deref() == Ok("1") {
                payload.uuid()
            } else {
                &id
            }
        );
        return std::process::ExitCode::SUCCESS;
    }
    if std::env::var("HORIZON_PROBE_GLOBAL_PAUSE").as_deref() == Ok("1") {
        let mut app = create().unwrap();
        app.boot().await.unwrap();
        app.make::<openrust_horizon::Repository>()
            .await
            .unwrap()
            .pause()
            .await
            .unwrap();
        return std::process::ExitCode::SUCCESS;
    }
    run_application(create).await
}
