use openrust::support::facades::Facade as _;
use openrust::{
    console::{
        Command, CommandDefinition, CommandRegistration, ConsoleError, ConsoleFuture, Input,
        MissingInputPrompt, Output,
    },
    contracts::console::{Isolatable, PromptsForMissingInput},
    foundation::{Application, configuration::Routing},
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Mail {
    definition: CommandDefinition,
    prompted: Arc<AtomicUsize>,
    text: bool,
}
impl Mail {
    fn new(name: &'static str, text: bool, prompted: Arc<AtomicUsize>) -> Self {
        let mut definition =
            CommandDefinition::from_signature("prompt {user : recipient ID} {items*} {--queue}")
                .unwrap();
        if name != "prompt" {
            definition = CommandDefinition::new(definition_syntax(name));
        }
        Self {
            definition,
            prompted,
            text,
        }
    }
}
fn definition_syntax(name: &'static str) -> clap::Command {
    clap::Command::new(name)
        .arg(clap::Arg::new("user").required(true).help("recipient ID"))
        .arg(
            clap::Arg::new("items")
                .required(true)
                .num_args(1..)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("queue")
                .long("queue")
                .action(clap::ArgAction::SetTrue),
        )
}
impl Command for Mail {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.stdout.push_str(&serde_json::to_string(&json!({"arguments":input.arguments(),"queue":input.option("queue"),"prompted":self.prompted.load(Ordering::SeqCst)})).unwrap());
            Ok(0)
        })
    }
}
impl Isolatable for Mail {
    fn isolatable_id(&self, input: &Input) -> Result<Option<String>, ConsoleError> {
        Ok(input
            .argument("user")
            .and_then(|value| value.as_str().map(str::to_owned)))
    }
}
impl PromptsForMissingInput for Mail {
    fn prompt_for_missing_arguments_using(&self) -> BTreeMap<String, MissingInputPrompt> {
        if self.text {
            return BTreeMap::from([(
                "items".into(),
                MissingInputPrompt::text("Choose item", "e.g. first"),
            )]);
        }
        let user = self.prompted.clone();
        let items = self.prompted.clone();
        BTreeMap::from([
            (
                "user".into(),
                MissingInputPrompt::custom(move |_, input, _| {
                    let count = user.clone();
                    Box::pin(async move {
                        assert!(input.argument("user").is_none_or(|v| v.is_null()));
                        input.set_option("queue", json!(true))?;
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(vec!["0".into()])
                    })
                }),
            ),
            (
                "items".into(),
                MissingInputPrompt::custom(move |_, input, _| {
                    let count = items.clone();
                    Box::pin(async move {
                        assert!(input.argument("user").is_some());
                        if input.argument("user") == Some(json!("0")) {
                            assert_eq!(input.option("queue"), Some(json!(true)));
                        }
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(vec!["first".into(), "two words".into()])
                    })
                }),
            ),
        ])
    }
    fn after_prompting_for_missing_arguments<'a>(
        &'a self,
        _: &'a Application,
        input: &'a mut Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<(), ConsoleError>> {
        Box::pin(async move {
            input.set_option("queue", json!(true))?;
            self.prompted.fetch_add(10, Ordering::SeqCst);
            Ok(())
        })
    }
}
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
    ]))
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(CommandRegistration::new(
        CommandRegistration::new(Mail::new("wrapped:prompt", false, Arc::default()))
            .prompting()
            .isolatable(),
    )))?;

    Artisan::add(Box::new(
        CommandRegistration::new(Fault {
            definition: CommandDefinition::from_signature("failure:prompt {user}").unwrap(),
        })
        .prompting()
        .isolatable(),
    ))?;

    Artisan::add(Box::new(
        CommandRegistration::new(Mail::new("prompt", false, Arc::default()))
            .prompting()
            .isolatable(),
    ))?;
    Artisan::add(Box::new(
        CommandRegistration::new(Mail::new("text:prompt", true, Arc::default())).prompting(),
    ))
}
fn app(path: &Path) -> Application {
    let application = Application::configure(path.into())
        .with_config(config)
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    application
        .container()
        .instance(Arc::new(FailureCount(AtomicUsize::new(0))))
        .unwrap();
    application
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/prompt", || async {
        let mut output = Output::default();
        Artisan::call("prompt", &["--isolated=17".into()], &mut output)
            .await
            .unwrap();
        output.stdout
    })?;
    Route::get("/prompt-failure", || async {
        let mut output = Output::default();
        Artisan::call("prompt", &["--no-interaction".into()], &mut output)
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        Ok::<_, openrust::http::HttpError>(output.stdout)
    })
}
#[tokio::test]
async fn generic_registration_prompts_missing_fields_and_preserves_isolation_and_hooks() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    openrust::support::facades::App::set_facade_application(Some(&application), async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("prompt", &["--isolated".into()], &mut output)
                .await
                .unwrap(),
            0
        );
        let result: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(result["arguments"]["user"], "0");
        assert_eq!(result["arguments"]["items"], json!(["first", "two words"]));
        assert_eq!(result["queue"], true);
        assert_eq!(result["prompted"], 12);
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("prompt", &["42", "supplied"].map(Into::into), &mut output)
                .await
                .unwrap(),
            0
        );
        let result: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(result["queue"], false);
        assert_eq!(result["prompted"], 12);
        for args in [
            vec!["--no-interaction"],
            vec!["--unknown"],
            vec!["42", "--no-interaction"],
        ] {
            assert!(
                Artisan::call(
                    "prompt",
                    &args.into_iter().map(Into::into).collect::<Vec<_>>(),
                    &mut Output::default()
                )
                .await
                .is_err()
            );
        }
        let mut output = Output::default();
        Artisan::call("prompt", &["42".into()], &mut output)
            .await
            .unwrap();
        let result: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(result["prompted"], 23);
        let mut help = Output::default();
        Artisan::call("help", &["prompt".into()], &mut help)
            .await
            .unwrap();
        assert!(help.stdout.contains("<user>"), "{}", help.stdout);
    })
    .await;
}
#[tokio::test]
async fn real_http_and_independent_artisan_prompt_and_recover_after_noninteractive_failure() {
    if let Ok(root) = std::env::var("PROMPT_CHILD_ROOT") {
        let code = app(Path::new(&root))
            .handle_command(&["prompt", "--isolated"].map(Into::into))
            .await
            .unwrap();
        assert_eq!(code, 0);
        return;
    }
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    let root = tempfile::tempdir().unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app(root.path()));
    kernel.bootstrap().await.unwrap();
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    for path in ["prompt", "prompt-failure", "prompt"] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}/{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        if path == "prompt-failure" {
            assert_eq!(response.status(), 500);
        } else {
            assert_eq!(response.status(), 200);
            let result: Value = response.json().await.unwrap();
            assert_eq!(result["queue"], true);
            assert_eq!(result["arguments"]["user"], "0");
        }
    }
    server.abort();
    let _ = server.await;
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "real_http_and_independent_artisan_prompt_and_recover_after_noninteractive_failure",
            "--nocapture",
        ])
        .env("PROMPT_CHILD_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("two words"));
}
#[tokio::test]
#[ignore = "由真实终端验收脚本调用"]
async fn terminal_prompt_fixture() {
    let Ok(root) = std::env::var("PROMPT_TTY_ROOT") else {
        return;
    };
    let args = std::env::var("PROMPT_TTY_ARGS").unwrap_or_else(|_| "text:prompt".into());
    let code = app(Path::new(&root))
        .handle_command(&args.split_whitespace().map(Into::into).collect::<Vec<_>>())
        .await
        .unwrap();
    let expected = std::env::var("PROMPT_EXPECT_CODE")
        .unwrap_or_else(|_| "0".into())
        .parse::<i32>()
        .unwrap();
    assert_eq!(code, expected);
}

struct Fault {
    definition: CommandDefinition,
}
struct FailureCount(AtomicUsize);
impl Command for Fault {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output
                .stdout
                .push_str(input.argument("user").unwrap().as_str().unwrap());
            Ok(0)
        })
    }
}
impl Isolatable for Fault {}
impl PromptsForMissingInput for Fault {
    fn prompt_for_missing_arguments_using(&self) -> BTreeMap<String, MissingInputPrompt> {
        BTreeMap::from([(
            "user".into(),
            MissingInputPrompt::custom(move |app, _, _| {
                Box::pin(async move {
                    let calls = app
                        .make::<FailureCount>()
                        .await
                        .map_err(ConsoleError::from_source)?;
                    match calls.0.fetch_add(1, Ordering::SeqCst) {
                        0 => Err(ConsoleError::from_source(std::io::Error::other(
                            "prompt-source",
                        ))),
                        1 => Ok(Vec::new()),
                        2 => {
                            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                            Ok(vec!["late".into()])
                        }
                        _ => Ok(vec!["recovered".into()]),
                    }
                })
            }),
        )])
    }
}
#[tokio::test]
async fn callbacks_keep_error_sources_validate_empty_answers_and_cancel_without_leaking_locks() {
    use std::error::Error as _;
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    openrust::support::facades::App::set_facade_application(Some(&application), async {
        let args = ["--isolated".into()];
        let error = Artisan::call("failure:prompt", &args, &mut Output::default())
            .await
            .unwrap_err();
        assert!(error.source().unwrap().is::<std::io::Error>());
        let error = Artisan::call("failure:prompt", &args, &mut Output::default())
            .await
            .unwrap_err();
        assert!(error.source().unwrap().is::<clap::Error>(), "{error:?}");
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(30),
                Artisan::call("failure:prompt", &args, &mut Output::default())
            )
            .await
            .is_err()
        );
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("failure:prompt", &args, &mut output)
                .await
                .unwrap(),
            0
        );
        assert_eq!(output.stdout, "recovered");
        let mut lock = openrust::support::facades::Cache::lock(
            "framework/command-failure:prompt",
            Some(std::time::Duration::from_secs(1)),
        )
        .unwrap();
        assert!(lock.get().await.unwrap());
        lock.release().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn actual_prompt_trait_matches_fixed_source_callback_and_after_hook_vectors() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/prompting-oracle.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let root = tempfile::tempdir().unwrap();
        let mut application = app(root.path());
        application.boot().await.unwrap();
        openrust::support::facades::App::set_facade_application(Some(&application), async {
            let mut args = Vec::new();
            if let Some(user) = case["supplied"].get("user") {
                args.push(user.as_str().unwrap().into());
            }
            if let Some(items) = case["supplied"].get("items").and_then(Value::as_array) {
                args.extend(items.iter().map(|v| v.as_str().unwrap().into()));
            }
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("prompt", &args, &mut output).await.unwrap(),
                0
            );
            let mut result: Value = serde_json::from_str(&output.stdout).unwrap();
            result["arguments"]
                .as_object_mut()
                .unwrap()
                .remove("command");
            assert_eq!(result["arguments"], case["arguments"]);
            assert_eq!(result["queue"], case["queue"]);
            assert_eq!(result["prompted"], case["prompted"]);
        })
        .await;
    }
}

#[tokio::test]
async fn nested_registrations_retain_markers_and_lock_ids_use_prompted_arguments() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    openrust::support::facades::App::set_facade_application(Some(&application), async {
        let mut lock = openrust::support::facades::Cache::lock(
            "framework/command-wrapped:prompt-0",
            Some(std::time::Duration::from_secs(10)),
        )
        .unwrap();
        assert!(lock.get().await.unwrap());
        let args = ["--isolated=17".into()];
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("wrapped:prompt", &args, &mut output)
                .await
                .unwrap(),
            17
        );
        assert!(output.stdout.contains("already running"));
        lock.release().await.unwrap();
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("wrapped:prompt", &args, &mut output)
                .await
                .unwrap(),
            0
        );
        let result: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(result["arguments"]["user"], "0");
        assert_eq!(result["queue"], true);
    })
    .await;
}
