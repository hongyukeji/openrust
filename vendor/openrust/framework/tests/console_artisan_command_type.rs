use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing},
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Artisan, Facade as _, Route},
    },
};
use serde_json::{Value, json};

struct Sender(&'static str);

struct Report {
    definition: CommandDefinition,
    calls: AtomicUsize,
}

impl Report {
    fn new() -> Self {
        Self {
            definition: CommandDefinition::from_signature("reports:notify {user} {--urgent}")
                .unwrap()
                .with_description("Send a report notification"),
            calls: AtomicUsize::new(0),
        }
    }
}

impl Command for Report {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let sender = app
                .make::<Sender>()
                .await
                .map_err(ConsoleError::from_source)?;
            let user = input.matches().get_one::<String>("user").unwrap();
            let calls = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            input
                .io(output)
                .line(&format!("{}:{user}:{calls}", sender.0))?;
            Ok(if input.matches().get_flag("urgent") {
                7
            } else {
                0
            })
        })
    }
}

struct Services {
    owner: &'static str,
    creates: Arc<AtomicUsize>,
}

impl ServiceProvider for Services {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            let creates = self.creates.clone();
            app.container().instance(Arc::new(Sender(self.owner)))?;
            app.container()
                .bind_async::<Report, _>(move |_| {
                    let creates = creates.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        creates.fetch_add(1, Ordering::SeqCst);
                        Ok(Arc::new(Report::new()))
                    })
                })
                .await?;
            Ok(())
        })
    }
}

fn application(path: &std::path::Path, owner: &'static str) -> (Application, Arc<AtomicUsize>) {
    let creates = Arc::new(AtomicUsize::new(0));
    let app = Application::configure(path.into())
        .with_command::<Report>()
        .with_command::<Report>()
        .with_providers(vec![Box::new(Services {
            owner,
            creates: creates.clone(),
        })])
        .with_routing(Routing::default().web(routes))
        .create()
        .unwrap();
    (app, creates)
}

#[tokio::test]
async fn command_type_resolution_reuses_one_instance_across_name_type_and_help() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/artisan-command-type-oracle.json"
    ))
    .unwrap();
    for owner in ["A", "B"] {
        let root = tempfile::tempdir().unwrap();
        let (mut app, creates) = application(root.path(), owner);
        app.boot().await.unwrap();
        assert_eq!(creates.load(Ordering::SeqCst), 0);
        App::set_facade_application(Some(&app), async {
            for case in cases.iter().filter(|row| row["owner"] == owner) {
                let mut output = Output::default();
                let mut args = vec![case["user"].as_str().unwrap().into(), "--no-ansi".into()];
                if case["urgent"] == true {
                    args.push("--urgent".into());
                }
                let status = if case["by_type"] == true {
                    Artisan::call_type::<Report>(&args, &mut output).await
                } else {
                    Artisan::call("reports:notify", &args, &mut output).await
                }
                .unwrap();
                assert_eq!(case["output"], output.stdout);
                assert_eq!(case["status"], status);
                assert_eq!(creates.load(Ordering::SeqCst), 1);
                let mut help = Output::default();
                Artisan::call_type::<Report>(&["--help".into(), "--no-ansi".into()], &mut help)
                    .await
                    .unwrap();
                assert!(help.stdout.contains(case["description"].as_str().unwrap()));
                assert!(help.stdout.contains("--urgent"));
            }
        })
        .await;
    }
}

#[tokio::test]
async fn unregistered_types_and_missing_bindings_propagate_distinct_errors() {
    let root = tempfile::tempdir().unwrap();
    for configure in [false, true] {
        let mut builder = Application::configure(root.path().into());
        if configure {
            builder = builder.with_command::<Report>();
        }
        let mut app = builder.create().unwrap();
        app.boot().await.unwrap();
        let mut output = Output::default();
        App::set_facade_application(Some(&app), async {
            let error = Artisan::call_type::<Report>(&["Ada".into()], &mut output)
                .await
                .unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains(if configure {
                    "command registration failed"
                } else {
                    "未注册命令类型"
                }),
                "{message}"
            );
        })
        .await;
        assert!(output.stdout.is_empty());
    }
}

#[test]
#[ignore = "由父消费者执行实际 Artisan"]
fn command_type_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let (app, _) = application(root.path(), "CLI");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let urgent = std::env::var_os("ARTISAN_TYPE_URGENT").is_some();
    let mut args = vec!["reports:notify".into(), "Ada".into(), "--no-ansi".into()];
    if urgent {
        args.push("--urgent".into());
    }
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_runs_container_command_and_uses_its_exit_code() {
    for (urgent, code) in [(false, 0), (true, 7)] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "command_type_cli_driver",
            "--ignored",
            "--nocapture",
        ]);
        command.env_remove("ARTISAN_TYPE_URGENT");
        if urgent {
            command.env("ARTISAN_TYPE_URGENT", "1");
        }
        let result = command.output().unwrap();
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("CLI:Ada:1"));
    }
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/report", || async {
        let mut output = Output::default();
        let status = Artisan::call_type::<Report>(&["User".into(), "--urgent".into()], &mut output)
            .await
            .map_err(|error| openrust::http::HttpError::Internal {
                source: Box::new(error),
            })?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"code":status,"output":output.stdout}),
        )
    })?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_type_calls_share_one_resolved_command_per_application() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let mut handles = Vec::new();
    let mut requests = tokio::task::JoinSet::new();
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut factories = Vec::new();
    for (owner, root) in ["A", "B"].into_iter().zip(&roots) {
        let (app, creates) = application(root.path(), owner);
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        handles.push(tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        }));
        factories.push(creates);
        for _ in 0..6 {
            requests.spawn(async move {
                let response = reqwest::get(format!("http://{address}/report"))
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let json: Value = response.json().await.unwrap();
                assert_eq!(json["code"], 7);
                let raw = json["output"].as_str().unwrap();
                let prefix = format!("{owner}:User:");
                let count = raw
                    .trim()
                    .strip_prefix(&prefix)
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                (owner, count)
            });
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    while let Some(result) = requests.join_next().await {
        assert!(seen.insert(result.unwrap()));
    }
    assert_eq!(seen.len(), 12);
    assert!(
        factories
            .iter()
            .all(|count| count.load(Ordering::SeqCst) == 1)
    );
    for handle in handles {
        handle.abort();
        let _ = handle.await;
    }
}
