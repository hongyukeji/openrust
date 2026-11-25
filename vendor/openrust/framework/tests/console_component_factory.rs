use openrust::console::{
    CommandDefinition, ConsoleError, ConsoleInputStream, Output, OutputStyle, OutputVerbosity,
    view::components::Factory,
};
use serde_json::Value;
use std::sync::Arc;
async fn invoke(style: &mut OutputStyle<'_>, case: &Value) -> Result<Value, ConsoleError> {
    let label = case["label"].as_str().unwrap();
    let default = case.get("default").filter(|v| !v.is_null()).cloned();
    match case["kind"].as_str().unwrap() {
        "Ask" => {
            if default.is_none() && case["multiline"] == false {
                Factory::new(style).ask(label).await
            } else {
                Factory::new(style)
                    .ask_with(label, default, case["multiline"].as_bool().unwrap())
                    .await
            }
        }
        "AskWithCompletion" => {
            let choices = case["choices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap());
            if default.is_none() {
                Factory::new(style)
                    .ask_with_completion(label, choices)
                    .await
            } else {
                Factory::new(style)
                    .ask_with_completion_with(label, choices, default)
                    .await
            }
        }
        "Confirm" => {
            if case["default"] == false {
                Factory::new(style).confirm(label).await.map(Value::Bool)
            } else {
                Factory::new(style)
                    .confirm_with(label, true)
                    .await
                    .map(Value::Bool)
            }
        }
        "Secret" => {
            if case["fallback"] == false {
                Factory::new(style).secret_with(label, false).await
            } else {
                Factory::new(style).secret(label).await
            }
        }
        "Choice" => {
            let attempts = case["attempts"]
                .as_u64()
                .map(|v| usize::try_from(v).unwrap());
            let multiple = case["multiple"].as_bool().unwrap();
            if let Some(values) = case["choices"].as_array() {
                if default.is_none() && attempts.is_none() && !multiple {
                    Factory::new(style)
                        .choice(label, values.iter().cloned())
                        .await
                } else {
                    Factory::new(style)
                        .choice_with(label, values.iter().cloned(), default, attempts, multiple)
                        .await
                }
            } else {
                Factory::new(style)
                    .choice_pairs_with(
                        label,
                        case["choices"]
                            .as_object()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| (k.as_str(), v.clone())),
                        default,
                        attempts,
                        multiple,
                    )
                    .await
            }
        }
        _ => unreachable!(),
    }
}
#[test]
#[ignore = "由父消费者设置真实COLUMNS并调用全部组件类型"]
fn components_matrix_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/factory-questions-oracle.json"
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
#[tokio::test]
async fn replacing_ordinary_output_does_not_rebind_components_or_their_input() {
    use serde_json::json;
    let definition = CommandDefinition::from_signature("factory:replace").unwrap();
    let mut first = definition.parse(&[]).unwrap();
    first.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"original\n".to_vec()),
    ))));
    let mut second = definition.parse(&[]).unwrap();
    second.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"replacement\n".to_vec()),
    ))));
    let mut original = Output::default();
    let mut replacement = Output::default();
    let mut third = Output::default();
    {
        let mut io = first.io(&mut original);
        io.set_output(OutputStyle::new(&second, &mut replacement));
        io.set_input(&second);
        io.line("ordinary replacement").unwrap();
        assert_eq!(
            io.output_components().ask("Name").await.unwrap(),
            json!("original")
        );
        io.set_output(OutputStyle::new(&second, &mut third));
        io.line("third output").unwrap();
        io.components().info("original component").unwrap();
    }
    let source: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/factory-rebinding-oracle.json"
    ))
    .unwrap();
    assert_eq!(original.stdout, source["original"]);
    assert_eq!(replacement.stdout, source["replacement"]);
    assert_eq!(third.stdout, source["third"]);
    assert!(original.stdout.contains("Name"));
    assert!(original.stdout.contains("original component"));
    assert!(!original.stdout.contains("ordinary replacement"));
    assert_eq!(replacement.stdout, "ordinary replacement\n");
    assert_eq!(third.stdout, "third output\n");
}
#[test]
fn raw_output_supports_display_but_reports_missing_question_input() {
    let mut output = Output::default();
    let mut factory = Factory::new(&mut output);
    factory.info("Saved").unwrap();
    factory.success("Done").unwrap();
    factory.warn("Careful").unwrap();
    factory.error("Failed").unwrap();
    factory.line("info", "Line").unwrap();
    factory.alert("Alert").unwrap();
    factory.bullet_list(["One", "Two"]).unwrap();
    factory.two_column_detail("Key", Some("Value")).unwrap();
    factory.task("Task").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let error = rt.block_on(factory.ask("Missing input")).unwrap_err();
    assert!(error.is_runtime());
    assert!(error.to_string().contains("问答样式"));
    for text in [
        "Saved", "Done", "Careful", "Failed", "Line", "ALERT", "One", "Two", "Key", "Value", "Task",
    ] {
        assert!(output.stdout.contains(text), "{text}");
    }
    assert!(!output.stdout.contains("Missing input"));
}
#[test]
#[ignore = "由父消费者固定终端宽度后验收显示工厂"]
fn display_factory_matrix_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/factory-display-oracle.json"
    ))
    .unwrap();
    for case in &cases {
        let input = CommandDefinition::from_signature("factory:display")
            .unwrap()
            .parse(&[])
            .unwrap();
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        output.set_verbosity(match case["level"].as_u64().unwrap() {
            8 => OutputVerbosity::Silent,
            16 => OutputVerbosity::Quiet,
            64 => OutputVerbosity::Verbose,
            128 => OutputVerbosity::VeryVerbose,
            256 => OutputVerbosity::Debug,
            _ => OutputVerbosity::Normal,
        });
        let mut style = OutputStyle::new(&input, &mut output);
        let mut f = Factory::new(&mut style);
        match case["kind"].as_str().unwrap() {
            "info" => f.info("Saved"),
            "success" => f.success("Saved"),
            "warn" => f.warn("Saved"),
            "error" => f.error("Saved"),
            "line" => f.line("info", "Saved"),
            "alert" => f.alert("Saved"),
            "bulletList" => f.bullet_list(["One", "Two"]),
            "twoColumnDetail" => f.two_column_detail("Key", Some("Value")),
            "task" => f.task("Saved"),
            _ => unreachable!(),
        }
        .unwrap();
        assert_eq!(
            style.new_lines_written(),
            case["new_lines"].as_u64().unwrap() as usize,
            "{case}"
        );
        assert_eq!(output.stdout, case["output"], "{case}");
        assert!(output.stderr.is_empty());
    }
    println!("FACTORY_DISPLAY_OK:{}", cases.len());
}
#[test]
fn display_factory_matches_fixed_source() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "display_factory_matrix_driver",
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
    assert!(String::from_utf8_lossy(&result.stdout).contains("FACTORY_DISPLAY_OK:108"));
}

async fn run_default(
    style: &mut (impl openrust::console::view::components::ComponentOutput + ?Sized),
    kind: &str,
    callback: bool,
    invalid: bool,
) -> Result<Value, ConsoleError> {
    use openrust::console::CompletionCallback;
    match kind {
        "Ask" => Factory::new(style).ask("Name").await,
        "AskMultiline" => Factory::new(style).ask_with("Name", None, true).await,
        "AskWithCompletion" if callback => {
            Factory::new(style)
                .ask_with_completion_callback(
                    "Name",
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
                    }),
                    None,
                )
                .await
        }
        "AskWithCompletion" => {
            Factory::new(style)
                .ask_with_completion("Name", ["green", "gray"])
                .await
        }
        "Confirm" => Factory::new(style)
            .confirm("Continue")
            .await
            .map(Value::Bool),
        "Secret" => Factory::new(style).secret("Password").await,
        "SecretStrict" => Factory::new(style).secret_with("Password", false).await,
        "Choice" if invalid => {
            Factory::new(style)
                .choice("Color", Vec::<String>::new())
                .await
        }
        "Choice" => Factory::new(style).choice("Color", ["red", "green"]).await,
        _ => unreachable!(),
    }
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
    let rows:Vec<Value>=serde_json::from_str(include_str!("../../../../.agents/project/audits/artisan-tutorial/question-components-console-oracle.json")).unwrap();
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
fn explicit_options_reach_each_selected_display_component() {
    use openrust::console::{
        OutputFormat, OutputOptions,
        view::{TaskResult, components::*},
    };
    for format in [OutputFormat::Normal, OutputFormat::Raw, OutputFormat::Plain] {
        let options = OutputOptions {
            format,
            verbosity: Some(OutputVerbosity::Quiet),
        };
        let mut actual = Output::default();
        let mut expected = Output::default();
        actual.set_decorated(true);
        expected.set_decorated(true);
        actual.set_verbosity(OutputVerbosity::Quiet);
        expected.set_verbosity(OutputVerbosity::Quiet);
        let mut factory = Factory::new(&mut actual);
        factory.info_with("Info", options).unwrap();
        Info::new(&mut expected)
            .render_with("Info", options)
            .unwrap();
        factory.success_with("Success", options).unwrap();
        Success::new(&mut expected)
            .render_with("Success", options)
            .unwrap();
        factory.warn_with("Warn", options).unwrap();
        Warn::new(&mut expected)
            .render_with("Warn", options)
            .unwrap();
        factory.error_with("Error", options).unwrap();
        Error::new(&mut expected)
            .render_with("Error", options)
            .unwrap();
        factory.line_with("info", "Line", options).unwrap();
        Line::new(&mut expected)
            .render_with("info", "Line", options)
            .unwrap();
        factory.alert_with("Alert", options).unwrap();
        Alert::new(&mut expected)
            .render_with("Alert", options)
            .unwrap();
        factory.bullet_list_with(["One", "Two"], options).unwrap();
        BulletList::new(&mut expected)
            .render_with(["One", "Two"], options)
            .unwrap();
        factory
            .two_column_detail_with("Key", Some("Value"), options)
            .unwrap();
        TwoColumnDetail::new(&mut expected)
            .render_with("Key", Some("Value"), options)
            .unwrap();
        factory.task_with("Task", options).unwrap();
        Task::new(&mut expected)
            .render_with("Task", options)
            .unwrap();
        assert_eq!(actual.stdout, expected.stdout);
        assert!(!actual.stdout.is_empty());
        Factory::new(&mut actual)
            .task_callback_with("Callback", |_| Ok(TaskResult::Skipped), options)
            .unwrap();
        assert!(actual.stdout.contains("SKIPPED"));
    }
}
