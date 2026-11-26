use std::{collections::BTreeMap, io::Write as _, path::Path, sync::Arc};

use openrust::{
    config::{ConfigError, Environment},
    console::{AsyncCommandCallback, ConsoleError},
    contracts::foundation::Application as _,
    database::migrations::MigrationEntry,
    foundation::{Application, configuration::Routing},
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};

#[path = "../../../../database/migrations/0001_01_01_000002_create_jobs_table.rs"]
mod queue_tables;

const MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "database/migrations/0001_01_01_000002_create_jobs_table.rs",
    &queue_tables::Definition,
)];

fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "dispatch:report {user} {--id=*} {--urgent} {--queue=default}",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let result = json!({
                    "user":input.argument("user"),
                    "id":input.option("id"),
                    "urgent":input.option("urgent"),
                    "queue":input.option("queue"),
                });
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(app.base_path().join("executed.jsonl"))
                    .map_err(ConsoleError::from_source)?;
                writeln!(file, "{result}").map_err(ConsoleError::from_source)?;
                input.io(output).line("worker input captured")?;
                Ok(if input.option("urgent") == Some(json!(true)) {
                    7
                } else {
                    0
                })
            })
        }),
    )?;
    Ok(())
}

fn config(root: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let backend = std::fs::read_to_string(root.join("backend")).unwrap();
    let database = match backend.as_str() {
        "mysql" => std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),
        "postgres" => std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),
        _ => format!("sqlite://{}?mode=rwc", root.join("jobs.sqlite").display()),
    };
    let redis = (backend == "redis").then(|| std::env::var("HORIZON_TEST_REDIS_URL").unwrap());
    let target = if backend == "redis" {
        json!({"driver":"redis","connection":"fixture","queue":"wrong","retry_after":90})
    } else {
        json!({"driver":"database","connection":"fixture","table":"jobs","queue":"wrong","retry_after":90})
    };
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"name":"Queued Artisan","key":"a".repeat(32)}),
        ),
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
        (
            "database".into(),
            json!({"default":"fixture","connections":{"fixture":{"url":database}},
                                  "redis":{"fixture":{"url":redis}}}),
        ),
        (
            "queue".into(),
            json!({"default":"sync","connections":{"sync":{"driver":"sync"}, "selected":target}}),
        ),
    ]))
}

fn app(root: &Path) -> Application {
    Application::configure(root.into())
        .with_config(config)
        .with_migrations(MIGRATIONS)
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap()
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/named", || async {
        Artisan::queue_named(
            "dispatch:report",
            &[
                ("user", json!("Ada Lovelace")),
                ("--id", json!([5, 13])),
                ("--urgent", json!(false)),
            ],
        )
        .on_connection("selected")
        .on_queue("artisan-named")
        .dispatch()
        .await
        .map_err(|error| openrust::http::HttpError::Internal {
            source: Box::new(error),
        })?;
        openrust::http::Response::json(openrust::http::StatusCode::ACCEPTED, json!(true))
    })?;
    Route::get("/legacy", || async {
        Artisan::queue(
            "dispatch:report",
            &["Léa".into(), "--id=7".into(), "--urgent".into()],
        )
        .on_connection("selected")
        .on_queue("artisan-named")
        .dispatch()
        .await
        .map_err(|error| openrust::http::HttpError::Internal {
            source: Box::new(error),
        })?;
        openrust::http::Response::json(openrust::http::StatusCode::ACCEPTED, json!(true))
    })?;
    Route::get("/invalid", || async {
        let result = Artisan::queue_named("dispatch:report", &[("--unknown", json!(true))])
            .on_connection("selected")
            .on_queue("artisan-named")
            .dispatch()
            .await;
        let status = if result.is_err() {
            openrust::http::StatusCode::UNPROCESSABLE_ENTITY
        } else {
            openrust::http::StatusCode::OK
        };
        openrust::http::Response::json(status, json!({"dispatch_ok":result.is_ok()}))
    })?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行实际 Artisan 及队列工作进程"]
fn queued_artisan_cli_driver() {
    let root = std::env::var_os("ARTISAN_QUEUE_FIXTURE").unwrap();
    let mode = std::env::var("ARTISAN_QUEUE_MODE").unwrap();
    let app = app(Path::new(&root));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = match mode.as_str() {
        "migrate" => vec!["migrate".into(), "--force".into()],
        "wrong" => vec![
            "queue:work".into(),
            "selected".into(),
            "--queue=wrong".into(),
            "--once".into(),
            "--sleep=0".into(),
        ],
        _ => vec![
            "queue:work".into(),
            "selected".into(),
            "--queue=artisan-named".into(),
            "--stop-when-empty".into(),
            "--sleep=0".into(),
            "--tries=1".into(),
        ],
    };
    let status = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    std::process::exit(status);
}

fn artisan(root: &Path, mode: &str) -> std::process::Output {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "queued_artisan_cli_driver",
            "--ignored",
            "--nocapture",
        ])
        .env("ARTISAN_QUEUE_FIXTURE", root)
        .env("ARTISAN_QUEUE_MODE", mode)
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(0), "{mode}: {child:?}");
    child
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_http_dispatch_and_independent_worker_keep_queue_arguments_and_destination() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }

    let source: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/artisan-queue-named-oracle.json"
    ))
    .unwrap();
    let expected = source
        .iter()
        .map(|row| row["observed"].clone())
        .collect::<Vec<_>>();
    assert!(source.iter().all(|row| row["connection"] == "selected"
        && row["queue"] == "artisan-named"
        && row["display"] == "dispatch:report"
        && row["worker_return"].is_null()));
    let mut backends = vec!["sqlite"];
    for (backend, variable) in [
        ("mysql", "OPENRUST_TEST_MYSQL_URL"),
        ("postgres", "OPENRUST_TEST_POSTGRES_URL"),
        ("redis", "HORIZON_TEST_REDIS_URL"),
    ] {
        if std::env::var_os(variable).is_some() {
            backends.push(backend);
        }
    }
    for backend in backends {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("backend"), backend).unwrap();
        if backend != "redis" {
            artisan(root.path(), "migrate");
        }
        let app = app(root.path());
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::new();
        for (route, expected) in [("invalid", 422), ("named", 202), ("legacy", 202)] {
            let response = client
                .get(format!("http://{address}/{route}"))
                .send()
                .await
                .unwrap();
            let status = response.status();
            assert_eq!(
                status,
                expected,
                "{backend}/{route}: {}",
                response.text().await.unwrap()
            );
        }
        let results = root.path().join("executed.jsonl");
        assert!(
            !results.exists(),
            "the selected connection must not use the sync default"
        );
        artisan(root.path(), "wrong");
        assert!(
            !results.exists(),
            "a different queue must leave these jobs untouched"
        );
        let worker = artisan(root.path(), "selected");
        let stdout = String::from_utf8_lossy(&worker.stdout);
        assert!(!stdout.contains("worker input captured"), "{stdout}");
        let rows: Vec<Value> = std::fs::read_to_string(&results)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows, expected, "{backend}");
        artisan(root.path(), "selected");
        assert_eq!(std::fs::read_to_string(results).unwrap().lines().count(), 2);
        server.abort();
        let _ = server.await;
        println!("已验真实 HTTP 和独立 queue:work：{backend}");
    }
}
