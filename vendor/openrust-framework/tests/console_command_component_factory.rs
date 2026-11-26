use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{
    console::{
        AsOutput as _, AsyncCommandCallback, ConsoleError, ConsoleFuture, Input, Output,
        OutputFormat, OutputOptions, OutputReference, OutputStyle,
        view::components::{
            CommandComponentFactory, ComponentArgument, ComponentContext, ComponentDispatch,
        },
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing},
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Artisan, Facade as _, Route},
    },
};
use serde_json::{Value, json};

struct ProbeProvider(Arc<ProbeFactory>);

impl ServiceProvider for ProbeProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container()
                .instance::<dyn CommandComponentFactory>(self.0.clone())?;
            Ok(())
        })
    }
}

struct ProbeFactory {
    owner: &'static str,
    builds: AtomicUsize,
}

impl ProbeFactory {
    fn new(owner: &'static str) -> Arc<Self> {
        Arc::new(Self {
            owner,
            builds: AtomicUsize::new(0),
        })
    }
}

impl CommandComponentFactory for ProbeFactory {
    fn make(
        &self,
        input: &Input,
        output: OutputReference<'_>,
    ) -> Result<Arc<dyn ComponentDispatch>, ConsoleError> {
        assert!(input.has_argument("mode"));
        assert!(!output.is_decorated());
        let builds = self.builds.fetch_add(1, Ordering::SeqCst) + 1;
        if self.owner == "error" || input.argument("mode") == Some(json!("failure")) {
            return Err(ConsoleError::new("factory unavailable"));
        }
        Ok(Arc::new(Tagged(format!("{}-{builds}", self.owner))))
    }
}

struct Tagged(String);

impl ComponentDispatch for Tagged {
    fn handles_call(&self, name: &str) -> bool {
        matches!(name, "answer") || name.eq_ignore_ascii_case("info")
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        parameters: Vec<ComponentArgument<'a>>,
        mut output: ComponentContext<'a>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            if name == "answer" {
                return Ok(json!(&self.0));
            }
            let [ComponentArgument::Value(Value::String(label))] = parameters.as_slice() else {
                return Err(ConsoleError::new("需要标签"));
            };
            output
                .as_output()
                .writeln(&format!("{}:{label}", self.0), OutputFormat::Raw)?;
            Ok(Value::Null)
        })
    }

    fn display_line(
        &self,
        name: &str,
        text: &str,
        options: OutputOptions,
        mut output: ComponentContext<'_>,
    ) -> Option<Result<(), ConsoleError>> {
        (name == "info").then(|| {
            output
                .as_output()
                .writeln(&format!("{}:{text}", self.0), options)
        })
    }
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "factory:probe {mode=once}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut replacement = Output::default();
                let mut io = input.io(output);
                let first = io.components().call("answer", vec![]).await.ok();
                io.components().info("one")?;
                io.line("ordinary")?;
                let replace = io.argument("mode") == Some(json!("replacement"));
                if replace {
                    io.set_output(OutputStyle::new(input, &mut replacement));
                    io.line("replacement")?;
                }
                io.output_components()
                    .call("info", vec!["two".into()])
                    .await?;
                assert_eq!(io.components().call("answer", vec![]).await.ok(), first);
                drop(io);
                assert_eq!(
                    replacement.stdout,
                    if replace { "replacement\n" } else { "" }
                );
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

fn application(path: &std::path::Path, factory: Option<Arc<ProbeFactory>>) -> Application {
    let mut builder = Application::configure(path.into())
        .with_routing(Routing::default().web(routes).commands(commands));
    if let Some(factory) = factory {
        builder = builder.with_providers(vec![Box::new(ProbeProvider(factory))]);
    }
    builder.create().unwrap()
}

#[tokio::test]
async fn command_factory_lifecycle_matches_fixed_container_construction() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/command-component-factory-oracle.json"
    ))
    .unwrap();
    for owner in ["A", "B", "none", "error"] {
        let root = tempfile::tempdir().unwrap();
        let factory = ProbeFactory::new(owner);
        let mut app = application(root.path(), (owner != "none").then(|| factory.clone()));
        app.boot().await.unwrap();
        App::set_facade_application(Some(&app), async {
            for case in cases.iter().filter(|case| case["owner"] == owner) {
                let mut output = Output::default();
                let mode = case["mode"].as_str().unwrap();
                let result = Artisan::call(
                    "factory:probe",
                    &[mode.into(), "--no-ansi".into()],
                    &mut output,
                )
                .await;
                assert_eq!(
                    result.as_ref().ok().copied(),
                    case["status"].as_i64().map(|v| v as i32),
                    "{case}"
                );
                if let Err(error) = result {
                    assert!(error.to_string().contains("factory unavailable"), "{error}");
                }
                assert_eq!(output.stdout, case["out"], "{case}");
                assert_eq!(
                    factory.builds.load(Ordering::SeqCst),
                    case["builds"].as_u64().unwrap() as usize
                );
            }
        })
        .await;
    }
}

#[test]
#[ignore = "由父消费者执行实际命令"]
fn command_factory_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = application(root.path(), Some(ProbeFactory::new("CLI")));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mode = std::env::var("COMPONENT_FACTORY_MODE").unwrap();
    let code = runtime
        .block_on(app.handle_command(&["factory:probe".into(), mode.into(), "--no-ansi".into()]))
        .unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_preserves_original_component_output_and_propagates_construction_error() {
    for (mode, expected_code) in [("once", 0), ("replacement", 0), ("failure", 1)] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "command_factory_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("COMPONENT_FACTORY_MODE", mode)
            .output()
            .unwrap();
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(child.status.code(), Some(expected_code), "{output}");
        if expected_code == 0 {
            assert!(
                output.contains("CLI-1:one\nordinary\nCLI-1:two"),
                "{output}"
            );
        } else {
            assert!(output.contains("factory unavailable"), "{output}");
            assert!(!output.contains("ordinary"), "{output}");
        }
    }
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/command-factory", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let mode = request
            .query(Some("mode"))?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "once".into());
        let mut output = Output::default();
        let result = Artisan::call(
            "factory:probe",
            &[mode.into(), "--no-ansi".into()],
            &mut output,
        )
        .await;
        openrust::http::Response::json(
            match result {
                Ok(0) => http::StatusCode::OK,
                _ => http::StatusCode::INTERNAL_SERVER_ERROR,
            },
            json!({"output":output.stdout}),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_commands_receive_a_fresh_factory_from_their_own_application() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let client = reqwest::Client::new();
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut servers = Vec::new();
    let mut calls = tokio::task::JoinSet::new();
    let mut factories = Vec::new();
    for (owner, root) in ["A", "B"].into_iter().zip(&roots) {
        let factory = ProbeFactory::new(owner);
        let mut kernel = Kernel::new(application(root.path(), Some(factory.clone())));
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        servers.push(tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        }));
        let response = client
            .get(format!("http://{address}/command-factory?mode=failure"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["output"], "");
        factories.push(factory);
        for _ in 0..6 {
            let client = client.clone();
            calls.spawn(async move {
                let response = client
                    .get(format!("http://{address}/command-factory?mode=replacement"))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let body: Value = response.json().await.unwrap();
                let output = body["output"].as_str().unwrap();
                let tag = output.lines().next().unwrap().strip_suffix(":one").unwrap();
                assert!(tag.starts_with(owner), "{output}");
                assert_eq!(output, format!("{tag}:one\nordinary\n{tag}:two\n"));
                tag.to_owned()
            });
        }
    }
    let mut tags = std::collections::BTreeSet::new();
    while let Some(tag) = calls.join_next().await {
        assert!(tags.insert(tag.unwrap()));
    }
    assert_eq!(tags.len(), 12);
    for factory in factories {
        assert_eq!(factory.builds.load(Ordering::SeqCst), 7);
    }
    for server in servers {
        server.abort();
        let _ = server.await;
    }
}
