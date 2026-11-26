use openrust::console::{
    CommandDefinition, ConsoleError, ConsoleInputStream, Output, OutputStyle, OutputVerbosity,
    view::components::Factory,
};
use serde_json::Value;
use std::sync::Arc;
async fn invoke(style: &mut OutputStyle<'_>, case: &Value) -> Result<Value, ConsoleError> {
    use openrust::console::view::components::ComponentArgument;
    let value = |key: &str| ComponentArgument::from(case[key].clone());
    let arguments = match case["kind"].as_str().unwrap() {
        "Ask" => vec![value("label"), value("default"), value("multiline")],
        "AskWithCompletion" => vec![value("label"), value("choices"), value("default")],
        "Confirm" => vec![value("label"), value("default")],
        "Secret" => vec![value("label"), value("fallback")],
        "Choice" => vec![
            value("label"),
            value("choices"),
            value("default"),
            value("attempts"),
            value("multiple"),
        ],
        _ => unreachable!(),
    };
    Factory::new(style)
        .call(case["kind"].as_str().unwrap(), arguments)
        .await
}
#[test]
#[ignore = "由父消费者设置真实COLUMNS并调用全部组件类型"]
fn components_matrix_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/factory-questions-oracle.json"
    ))
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = openrust::foundation::Application::configure(root.path().into())
        .create()
        .unwrap();
    rt.block_on(async {
        app.boot().await.unwrap();
        use openrust::support::facades::{App, Facade as _};
        App::set_facade_application(Some(&app), async {
            for case in &cases {
                let mut input = CommandDefinition::from_signature("components:questions")
                    .unwrap()
                    .parse(&[])
                    .unwrap();
                input
                    .set_interactive(case["interactive"].as_bool().unwrap())
                    .set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(case["input"].as_str().unwrap().as_bytes().to_vec()),
                    ))));
                let mut output = Output::default();
                output.set_decorated(case["decorated"].as_bool().unwrap());
                output.set_verbosity(match case["level"].as_u64().unwrap() {
                    8 => OutputVerbosity::Silent,
                    16 => OutputVerbosity::Quiet,
                    64 => OutputVerbosity::Verbose,
                    _ => OutputVerbosity::Normal,
                });
                let mut style = OutputStyle::new(&input, &mut output);
                assert!(style.get_question_helper().is_none());
                let result = invoke(&mut style, case).await;
                assert_eq!(
                    result.is_err(),
                    !case["error"].is_null(),
                    "{case}: {result:?}"
                );
                if let Ok(answer) = result {
                    assert_eq!(answer, case["answer"], "{case}");
                }
                assert!(
                    style.get_question_helper().is_some(),
                    "helper not restored: {case}"
                );
                assert_eq!(
                    style.new_lines_written(),
                    case["new_lines"].as_u64().unwrap() as usize,
                    "{case}"
                );
                assert_eq!(output.stdout, case["output"], "{case}");
                assert!(output.stderr.is_empty());
                assert_eq!(input.is_interactive(), case["interactive_after"], "{case}");
            }
        })
        .await;
    });
    println!("COMPONENT_QUESTION_CASES_OK:{}", cases.len());
}
#[test]
fn all_five_component_render_entries_match_fixed_source() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "components_matrix_driver",
            "--ignored",
            "--nocapture",
        ])
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("COMPONENT_QUESTION_CASES_OK:134"));
}

async fn run_default(
    style: &mut (impl openrust::console::view::components::ComponentOutput + ?Sized),
    kind: &str,
    callback: bool,
    invalid: bool,
) -> Result<Value, ConsoleError> {
    use openrust::console::{CompletionCallback, view::components::ComponentArgument as Arg};
    let (method, args) = match kind {
        "Ask" => ("ask", vec!["Name".into()]),
        "AskMultiline" => (
            "ask",
            vec!["Name".into(), Value::Null.into(), Value::Bool(true).into()],
        ),
        "AskWithCompletion" if callback => (
            "askWithCompletion",
            vec![
                "Name".into(),
                CompletionCallback::sync(|typed| {
                    if let Ok(path) = std::env::var("COMPONENT_TRACE") {
                        use std::io::Write as _;
                        let mut file = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)
                            .map_err(ConsoleError::from_source)?;
                        writeln!(file, "{}", serde_json::to_string(&typed).unwrap())
                            .map_err(ConsoleError::from_source)?;
                    }
                    if typed == "err" {
                        return Err(ConsoleError::new("completion failed"));
                    }
                    Ok(vec!["green".into(), "gray".into()])
                })
                .into(),
            ],
        ),
        "AskWithCompletion" => (
            "askWithCompletion",
            vec!["Name".into(), serde_json::json!(["green", "gray"]).into()],
        ),
        "Choice" => (
            "choice",
            vec![
                "Color".into(),
                if invalid {
                    serde_json::json!([])
                } else {
                    serde_json::json!(["red", "green"])
                }
                .into(),
            ],
        ),
        "Confirm" => ("confirm", vec!["Continue".into()]),
        "Secret" => ("secret", vec!["Password".into()]),
        "SecretStrict" => ("secret", vec!["Password".into(), Value::Bool(false).into()]),
        _ => unreachable!(),
    };
    let args: Vec<Arg<'_>> = args;
    Factory::new(style).call(method, args).await
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "factory:show {--kind=Ask} {--surface=derived} {--answer=} {--callback} {--invalid}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                use openrust::console::{AsOutput, ConsoleStyle, OutputFormat};
                let mut input = input.clone();
                if let Some(answer) = input
                    .option("answer")
                    .and_then(|v| v.as_str().map(str::to_owned))
                {
                    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(answer.into_bytes()),
                    ))));
                }
                let kind = input
                    .option("kind")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Ask".into());
                let surface = input
                    .option("surface")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "derived".into());
                let callback = input.option("callback").and_then(|v| v.as_bool()) == Some(true);
                let invalid = input.option("invalid").and_then(|v| v.as_bool()) == Some(true);
                let value = match surface.as_str() {
                    "base" => {
                        run_default(
                            &mut ConsoleStyle::new(&input, output),
                            &kind,
                            callback,
                            invalid,
                        )
                        .await?
                    }
                    "nested" => {
                        let mut parent = OutputStyle::new(&input, output);
                        run_default(
                            &mut OutputStyle::new(&input, &mut parent),
                            &kind,
                            callback,
                            invalid,
                        )
                        .await?
                    }
                    "stderr" => {
                        let mut style = OutputStyle::new(&input, output);
                        run_default(&mut style.get_error_style(), &kind, callback, invalid).await?
                    }
                    _ => {
                        run_default(
                            &mut OutputStyle::new(&input, output),
                            &kind,
                            callback,
                            invalid,
                        )
                        .await?
                    }
                };
                output.as_output().writeln(
                    format!("COMPONENT_VALUE:{}", serde_json::to_string(&value).unwrap()).as_str(),
                    OutputFormat::Raw,
                )?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由实际Artisan/PTY父消费者设置参数、输入和输出"]
fn factory_cli_driver() {
    let case: Value = serde_json::from_str(&std::env::var("FACTORY_CASE").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let mut args: Vec<std::ffi::OsString> = vec![
        "factory:show".into(),
        format!("--kind={}", case["kind"].as_str().unwrap()).into(),
        format!("--surface={}", case["surface"].as_str().unwrap()).into(),
        if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        }
        .into(),
    ];
    if case["callback"] == true {
        args.push("--callback".into());
    }
    if case["invalid"] == true {
        args.push("--invalid".into());
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn all_component_classes_in_real_artisan_match_source_physical_streams() {
    use std::io::Write as _;
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/question-components-console-oracle.json"
    ))
    .unwrap();
    for case in rows {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "factory_cli_driver", "--ignored", "--nocapture"])
            .env("COLUMNS", "80")
            .env("FACTORY_CASE", case.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(case["input"].as_str().unwrap().as_bytes())
            .unwrap();
        let result = process.wait_with_output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "{case}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stdout = stdout.strip_prefix("\nrunning 1 test\n").unwrap_or(&stdout);
        assert_eq!(stdout, case["stdout"], "{case}");
        assert_eq!(
            String::from_utf8(result.stderr).unwrap(),
            case["stderr"],
            "{case}"
        );
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/factory", component_response)?;
    Ok(())
}
fn component_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let kind = request
            .query(Some("kind"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Ask".into());
        let answer = request
            .query(Some("answer"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Guest".into());
        let mut args = vec![
            format!("--kind={kind}").into(),
            format!("--answer={answer}\n").into(),
        ];
        if request.query(Some("invalid"))?.is_some() {
            args.push("--invalid".into());
        }
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("factory:show", &args, &mut output).await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            serde_json::json!({"output":output.stdout,"error":output.stderr}),
        )
    })
}
#[tokio::test]
async fn actual_http_component_types_errors_recovery_and_parallel_commands() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(routes)
                .commands(commands),
        )
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
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
    for (kind, answer, expected) in [
        ("Ask", "reader", serde_json::json!("reader")),
        ("AskWithCompletion", "green", serde_json::json!("green")),
        ("Choice", "1", serde_json::json!("green")),
        ("Confirm", "yes", serde_json::json!(true)),
        ("Secret", "secret", serde_json::json!("secret")),
    ] {
        let response = reqwest::get(format!(
            "http://{address}/factory?kind={kind}&answer={answer}"
        ))
        .await
        .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert!(
            body["output"]
                .as_str()
                .unwrap()
                .contains(&format!("COMPONENT_VALUE:{expected}")),
            "{body}"
        );
    }
    let response = reqwest::get(format!("http://{address}/factory?kind=Choice&invalid=1"))
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..12 {
        requests.spawn(async move {
            let answer = format!("Recovered{i}");
            let response =
                reqwest::get(format!("http://{address}/factory?kind=Ask&answer={answer}"))
                    .await
                    .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("COMPONENT_VALUE:\"{answer}\""))
            );
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn task_callbacks_and_borrowed_output_keep_failure_and_formatter_errors() {
    use openrust::console::{AsOutput as _, OutputFormat, view::TaskResult};
    let mut output = Output::default();
    let mut f = Factory::new(&mut output);
    f.task_callback("Skipped", |o| {
        o.writeln("callback", OutputFormat::Raw)?;
        Ok(TaskResult::Skipped)
    })
    .unwrap();
    let error = f
        .task_callback::<Value>("Failure", |_| Err(ConsoleError::new("original error")))
        .unwrap_err();
    assert!(error.to_string().contains("original error"));
    f.get_output().write_raw("after").unwrap();
    f.as_output().write_raw(" stream").unwrap();
    assert!(output.stdout.contains("SKIPPED"));
    assert!(output.stdout.contains("FAIL"));
    assert!(output.stdout.ends_with("after stream"));
}
#[test]
#[ignore = "由父消费者固定动态工厂的终端宽度"]
fn dynamic_display_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/factory-display-oracle.json"
    ))
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    for case in &cases {
        let input = CommandDefinition::from_signature("factory:dynamic")
            .unwrap()
            .parse(&[])
            .unwrap();
        let mut out = Output::default();
        out.set_decorated(case["decorated"].as_bool().unwrap());
        out.set_verbosity(match case["level"].as_u64().unwrap() {
            8 => OutputVerbosity::Silent,
            16 => OutputVerbosity::Quiet,
            64 => OutputVerbosity::Verbose,
            128 => OutputVerbosity::VeryVerbose,
            256 => OutputVerbosity::Debug,
            _ => OutputVerbosity::Normal,
        });
        let mut style = OutputStyle::new(&input, &mut out);
        let args = match case["kind"].as_str().unwrap() {
            "line" => serde_json::json!(["info", "Saved"]),
            "bulletList" => serde_json::json!([["One", "Two"]]),
            "twoColumnDetail" => serde_json::json!(["Key", "Value"]),
            _ => serde_json::json!(["Saved"]),
        };
        let answer = rt
            .block_on(
                Factory::new(&mut style).call(
                    case["kind"].as_str().unwrap(),
                    args.as_array()
                        .unwrap()
                        .iter()
                        .cloned()
                        .map(Into::into)
                        .collect(),
                ),
            )
            .unwrap();
        assert_eq!(answer, Value::Null);
        assert_eq!(
            style.new_lines_written(),
            case["new_lines"].as_u64().unwrap() as usize,
            "{case}"
        );
        assert_eq!(out.stdout, case["output"], "{case}");
        assert!(out.stderr.is_empty());
    }
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/factory-dynamic-oracle.json"
    ))
    .unwrap();
    for case in &cases {
        let mut input = CommandDefinition::from_signature("factory:dynamic")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_interactive(false);
        let mut out = Output::default();
        let mut style = OutputStyle::new(&input, &mut out);
        let result = rt.block_on(
            Factory::new(&mut style).call(
                case["name"].as_str().unwrap(),
                case["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
            ),
        );
        assert_eq!(
            result.is_err(),
            !case["error"].is_null(),
            "{case}: {result:?}"
        );
        if let Ok(value) = result {
            assert_eq!(value, case["answer"], "{case}");
        }
        assert_eq!(out.stdout, case["output"], "{case}");
        // 错误参数在原生侧先校验，不复制参考解释器泄漏模板输出缓冲的缺陷。
        if case["leaked_stdout"] != "" {
            assert_eq!(case["error"], "TypeError");
            assert_eq!(case["args"], serde_json::json!([[1, 2]]));
        }
    }
    println!("DYNAMIC_FACTORY_OK:134");
}
#[test]
fn dynamic_factory_matches_source_names_arguments_and_display() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "dynamic_display_driver",
            "--ignored",
            "--nocapture",
        ])
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("DYNAMIC_FACTORY_OK:134"));
}
#[tokio::test]
async fn dynamic_task_calls_compiled_closures_once_and_finishes_before_error() {
    use openrust::console::{
        OutputFormat,
        view::{TaskResult, components::ComponentArgument as Arg},
    };
    let mut output = Output::default();
    let mut calls = 0;
    Factory::new(&mut output)
        .call(
            "task",
            vec![
                "Skipped".into(),
                Arg::task(|o| {
                    calls += 1;
                    o.writeln("inside", OutputFormat::Raw)?;
                    Ok(TaskResult::Skipped)
                }),
            ],
        )
        .await
        .unwrap();
    assert_eq!(calls, 1);
    assert!(output.stdout.contains("SKIPPED"));
    assert!(output.stdout.contains("inside"));
    let result = Factory::new(&mut output)
        .call(
            "task",
            vec![
                "Failure".into(),
                Arg::task::<Value>(|_| {
                    calls += 1;
                    Err(ConsoleError::new("task failed"))
                }),
            ],
        )
        .await;
    assert_eq!(calls, 2);
    assert!(result.unwrap_err().to_string().contains("task failed"));
    assert!(output.stdout.contains("FAIL"));
    let before = output.stdout.len();
    let invalid = Factory::new(&mut output)
        .call("task", vec!["Invalid".into(), Value::Bool(true).into()])
        .await;
    assert!(invalid.is_err());
    assert!(output.stdout[before..].contains("Invalid"));
    assert!(output.stdout[before..].contains("FAIL"));
    Factory::new(&mut output)
        .call("info", vec!["Recovered".into()])
        .await
        .unwrap();
    assert!(output.stdout.contains("Recovered"));
}
#[tokio::test]
async fn dynamic_question_cancellation_restores_helper_and_keeps_original_stream() {
    use openrust::console::{QuestionHelperBehavior, view::components::ComponentArgument as Arg};
    use tokio::io::AsyncWriteExt as _;
    struct Original;
    impl QuestionHelperBehavior for Original {
        fn get_name(&self) -> &str {
            "original"
        }
    }
    let mut input = CommandDefinition::from_signature("factory:cancel")
        .unwrap()
        .parse(&[])
        .unwrap();
    let (reader, mut writer) = tokio::io::duplex(128);
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        reader,
    ))));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let original: Arc<dyn QuestionHelperBehavior> = Arc::new(Original);
    style.set_question_helper(Some(original.clone()));
    let mut factory = Factory::new(&mut style);
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        factory.call("ask", vec![Arg::from("Pending")]),
    )
    .await;
    assert!(result.is_err());
    assert!(Arc::ptr_eq(
        &original,
        &style.get_question_helper().unwrap()
    ));
    writer.write_all(b"recovered\n").await.unwrap();
    assert_eq!(
        Factory::new(&mut style)
            .call("ask", vec!["Again".into()])
            .await
            .unwrap(),
        Value::String("recovered".into())
    );
    assert!(Arc::ptr_eq(
        &original,
        &style.get_question_helper().unwrap()
    ));
}
#[tokio::test]
async fn dynamic_output_options_are_checked_without_executing_invalid_callback_arguments() {
    use openrust::console::{
        OutputFormat, OutputOptions, view::components::ComponentArgument as Arg,
    };
    let mut output = Output::default();
    output.set_verbosity(OutputVerbosity::Quiet);
    let result = Factory::new(&mut output)
        .call(
            "INFO",
            vec![
                "Quiet".into(),
                OutputOptions {
                    format: OutputFormat::Plain,
                    verbosity: Some(OutputVerbosity::Quiet),
                }
                .into(),
            ],
        )
        .await
        .unwrap();
    assert!(result.is_null());
    assert!(output.stdout.contains("Quiet"));
    let before = output.stdout.clone();
    let mut calls = 0;
    assert!(
        Factory::new(&mut output)
            .call(
                "unknown",
                vec![Arg::task(|_| {
                    calls += 1;
                    Ok(true)
                })]
            )
            .await
            .is_err()
    );
    assert_eq!(calls, 0);
    assert_eq!(output.stdout, before);
    assert!(
        Factory::new(&mut output)
            .call("info", vec!["Invalid".into(), serde_json::json!(48).into()])
            .await
            .is_err()
    );
    assert_eq!(output.stdout, before);
}
