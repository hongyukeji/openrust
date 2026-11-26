use openrust::{
    console::{
        Application, AsyncCommandCallback, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        FormatterHelper, HelperNotFound, HelperSet, Input, Output, ProcessHelper,
    },
    foundation::{Application as Foundation, configuration::Routing},
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};
use std::{error::Error as _, sync::Arc};

fn only_formatter(alias: &str) -> HelperSet {
    let set = HelperSet::new();
    set.set(Arc::new(FormatterHelper::new()), Some(alias));
    set
}

fn names(set: &HelperSet) -> Vec<String> {
    set.get_iterator().map(|(name, _)| name).collect()
}

struct Report(CommandDefinition);

impl Report {
    fn new() -> Self {
        Self(CommandDefinition::new(
            clap::Command::new("helpers:report")
                .visible_alias("helpers:alias")
                .arg(clap::Arg::new("mode").default_value("report")),
        ))
    }
}

impl Command for Report {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _app: &'a Foundation,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let mut replay = input.with_shutdown(tokio_util::sync::CancellationToken::new());
            replay.set_argument("mode", ["secondary"])?;
            assert!(
                replay
                    .get_helper_set()
                    .unwrap()
                    .same_set(input.get_helper_set().unwrap())
            );
            let io = replay.io(output);
            let helper = io.get_helper("report")?;
            assert!(Arc::ptr_eq(&helper, &input.get_helper("formatter")?));
            let message = names(io.get_helper_set().unwrap()).join(",");
            output.comment(&message);
            Ok(0)
        })
    }
}

#[test]
fn registered_command_snapshots_and_replacements_match_fixed_source() {
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/default-helpers-oracle.json"
    ))
    .unwrap();
    let mut application = Application::new();
    let defaults = application.get_helper_set();
    assert_eq!(
        names(&defaults),
        ["formatter", "debug_formatter", "process", "question"]
    );
    assert_eq!(
        defaults
            .get_iterator()
            .map(|(key, helper)| (key, Value::String(helper.get_name().into())))
            .collect::<serde_json::Map<_, _>>(),
        *oracle["default"].as_object().unwrap()
    );
    assert_eq!(
        defaults.same_set(&application.get_helper_set()),
        oracle["stable_app_identity"]
    );
    assert_eq!(
        !defaults.same_set(&Application::new().get_helper_set()),
        oracle["separate_apps"]
    );
    assert!(defaults.same_set(&application.clone().get_helper_set()));

    let detached = Report::new().definition().parse(&[]).unwrap();
    assert_eq!(
        detached.get_helper_set().is_some(),
        oracle["detached_has_set"]
    );
    assert!(detached.get_helper("formatter").is_err());

    application.add_command(Box::new(Report::new())).unwrap();
    let set = application
        .get_command_helper_set("helpers:alias")
        .unwrap()
        .unwrap();
    assert_eq!(set.same_set(&defaults), oracle["first_inherits"]);
    let custom = only_formatter("report");
    application.set_helper_set(custom.clone());
    assert_eq!(
        application
            .get_command_helper_set("helpers:report")
            .unwrap()
            .unwrap()
            .same_set(&defaults),
        oracle["registered_keeps_previous_set"]
    );
    let second = application.command("helpers:second", |_| {}).unwrap();
    assert_eq!(
        second.get_helper_set().unwrap().same_set(&custom),
        oracle["new_command_inherits_override"]
    );
    assert_eq!(
        Arc::ptr_eq(
            &second.get_helper("formatter").unwrap(),
            &second.get_helper("report").unwrap()
        ),
        oracle["named_helper_is_alias_identity"]
    );
    second.set_helper_set(defaults.clone());
    assert!(second.get_helper_set().unwrap().same_set(&defaults));
    application
        .set_command_helper_set("helpers:alias", custom.clone())
        .unwrap();
    assert_eq!(
        application
            .get_command_helper_set("helpers:report")
            .unwrap()
            .unwrap()
            .same_set(&custom),
        oracle["command_override"]
    );
    let error = custom.get("process").err().unwrap();
    assert!(error.to_string().contains("process"));
    assert!(application.get_command_helper_set("missing").is_err());
}

fn commands() -> Result<(), ConsoleError> {
    let original = Artisan::get_helper_set()?;
    assert!(original.has("debug_formatter"));
    Artisan::command_with(
        "helpers:defaults",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let original = input.get_helper_set().unwrap();
                assert_eq!(
                    names(original),
                    ["formatter", "debug_formatter", "process", "question"]
                );
                let helper: Arc<dyn std::any::Any + Send + Sync> = input.get_helper("process")?;
                let helper = helper.downcast::<ProcessHelper>().unwrap();
                let generated = helper.must_run(output, ["git", "--version"]).await?;
                output.comment(String::from_utf8_lossy(generated.output()).trim());
                Ok(0)
            })
        }),
    )?;
    let overrides = only_formatter("report");
    Artisan::set_helper_set(overrides.clone())?;
    Artisan::add(Box::new(Report::new()))?;
    Artisan::command_with(
        "helpers:missing",
        AsyncCommandCallback::new(|_, input, _| {
            Box::pin(async move {
                let error = input.get_helper("process").err().unwrap();
                assert!(
                    error
                        .source()
                        .unwrap()
                        .downcast_ref::<HelperNotFound>()
                        .is_some()
                );
                Err(error)
            })
        }),
    )?;
    Artisan::set_command_helper_set("helpers:alias", only_formatter("report"))?;
    assert!(overrides.same_set(&Artisan::get_helper_set()?));
    Ok(())
}

#[test]
#[ignore = "由父消费者执行真实 Artisan 进程"]
fn default_helpers_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = Foundation::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        std::env::var("DEFAULT_HELPERS_COMMAND").unwrap().into(),
        "--no-ansi".into(),
    ];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let status = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(status);
}

#[test]
fn actual_artisan_registry_and_aliases_use_registered_helpers() {
    for (command, status, content) in [
        ("helpers:defaults", 0, "git version"),
        ("helpers:alias", 0, "formatter,report"),
        ("helpers:missing", 1, "process"),
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "default_helpers_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("DEFAULT_HELPERS_COMMAND", command)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(status), "{stdout}\n{stderr}");
        assert!(
            stdout.contains(content) || stderr.contains(content),
            "{stdout}\n{stderr}"
        );
    }
    assert!(Artisan::get_helper_set().is_err());
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/helpers", response)?;
    Ok(())
}

fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let command = match request
            .query(Some("mode"))?
            .as_ref()
            .and_then(Value::as_str)
        {
            Some("alias") => "helpers:alias",
            Some("missing") => "helpers:missing",
            _ => "helpers:defaults",
        };
        let mut output = Output::default();
        let result = Artisan::call(command, &[], &mut output).await;
        openrust::http::Response::json(
            if matches!(result, Ok(0)) {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"output": output.stdout}),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_default_and_command_specific_helpers_survive_failure() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Foundation::configure(root.path().into())
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
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
    for (mode, status, content) in [
        ("missing", 500, None),
        ("default", 200, Some("git version")),
        ("alias", 200, Some("formatter,report")),
    ] {
        let response = client
            .get(format!("http://{address}/helpers?mode={mode}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body: Value = response.json().await.unwrap();
        if let Some(content) = content {
            assert!(body["output"].as_str().unwrap().contains(content));
        }
    }
    server.abort();
    let _ = server.await;
}
