use openrust::console::{
    AsOutput, CommandDefinition, ConsoleError, ConsoleInputStream, Output, OutputReference,
    OutputStyle, OutputVerbosity, QuestionCallback, contracts::NewLineAware,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn parent_counts(output: &mut OutputReference<'_>) -> Vec<Value> {
    let mut counts = Vec::new();
    if let Some(view) = output.as_new_line_aware() {
        counts.push(json!([view.new_lines_written(), view.new_line_written()]));
        counts.extend(parent_counts(&mut output.get_output().unwrap()));
    }
    counts
}
async fn run_case(style: &mut OutputStyle<'_>, case: &Value) -> Value {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let calls = trace.clone();
    let mode = case["mode"].as_str().unwrap().to_owned();
    let callback_mode = mode.clone();
    let validator = QuestionCallback::sync(move |value| {
        calls.lock().unwrap().push(value.clone());
        match callback_mode.as_str() {
            "ask-console-runtime" => Err(ConsoleError::from_runtime_source(std::io::Error::other(
                "validator runtime",
            ))),
            "ask-user-runtime" => Err(ConsoleError::from_source(std::io::Error::other(
                "validator runtime",
            ))),
            "ask-retry" => {
                if value == "bad" {
                    Err(ConsoleError::new("Try again"))
                } else {
                    Ok(json!(value.as_str().unwrap().to_uppercase()))
                }
            }
            _ => Ok(json!({"value":value})),
        }
    });
    let default = case.get("default").and_then(Value::as_str);
    let result = match mode.as_str() {
        "confirm-basic" => style.confirm("Continue").await.map(|value| json!(value)),
        "choice-basic" => style.choice("Color", ["red", "green"]).await,
        "ask-null-validator" => style.ask_with_validator("Name", Some("Guest"), None).await,
        "hidden-null-validator" => style.ask_hidden_with("Password", None).await,
        "ask" => style.ask("Name").await.map(|value| json!(value)),
        "ask-default" => style
            .ask_with("Name", default)
            .await
            .map(|value| json!(value)),
        "ask-wrap" | "ask-retry" | "ask-user-runtime" | "ask-console-runtime" => {
            style
                .ask_with_validator("Name", default, Some(validator))
                .await
        }
        "hidden" => style.ask_hidden("Password").await.map(|value| json!(value)),
        "hidden-wrap" => style.ask_hidden_with("Password", Some(validator)).await,
        "confirm" => style
            .confirm_with("Continue", case["default"].as_bool().unwrap())
            .await
            .map(|value| json!(value)),
        "choice" => {
            style
                .choice_with(
                    "Color",
                    ["red", "green"],
                    case.get("default")
                        .filter(|value| !value.is_null())
                        .cloned(),
                    case["multiple"].as_bool().unwrap_or(false),
                )
                .await
        }
        _ => unreachable!(),
    };
    assert_eq!(result.is_err(), case["error"].is_string(), "{case}");
    let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
    counts.extend(parent_counts(style.get_output()));
    counts.reverse();
    json!({"value":result.ok(),"output":style.get_output().stdout,"counts":counts,"trace":*trace.lock().unwrap()})
}
#[test]
fn existing_style_question_helpers_match_fixed_source() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "style_questions_driver",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
#[test]
#[ignore = "由父消费者隔离终端与输入"]
fn style_questions_driver() {
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/style-questions-oracle.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let mut input = CommandDefinition::from_signature("style-questions")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_interactive(case["interactive"].as_bool().unwrap());
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            std::io::Cursor::new(case["answer"].as_str().unwrap().as_bytes().to_vec()),
        ))));
        let mut output = Output::default();
        output.set_verbosity(if case["level"] == 16 {
            OutputVerbosity::Quiet
        } else {
            OutputVerbosity::Normal
        });
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let mut section;
        let target: &mut dyn AsOutput = if case["section"] == true {
            section = output.section();
            &mut section
        } else {
            &mut output
        };
        let mut first = OutputStyle::new(&input, target);
        let actual = if case["depth"] == 2 {
            let mut second = OutputStyle::new(&input, &mut first);
            runtime.block_on(run_case(&mut second, case))
        } else {
            runtime.block_on(run_case(&mut first, case))
        };
        let expected = json!({"value":case["value"],"output":case["output"],"counts":case["counts"],"trace":case["trace"]});
        assert_eq!(
            input.is_interactive(),
            case["interactive_after"].as_bool().unwrap(),
            "interactive {index}"
        );
        if actual != expected {
            failures.push(format!(
                "case {index}: actual {actual:?}, expected {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn register_style_question_helpers() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:questions {--invalid}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let invalid = input.option("invalid").and_then(|v| v.as_bool()) == Some(true);
                let mut reading = input.clone();
                if std::env::var_os("STYLE_QUESTIONS_LIVE").is_none() {
                    reading.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(b"Ada\nsample-secret\n\n1\n".to_vec()),
                    ))));
                }
                let validator = QuestionCallback::sync(move |value| {
                    if invalid {
                        return Err(ConsoleError::from_runtime_source(std::io::Error::other(
                            "validator runtime",
                        )));
                    }
                    if value == "bad" {
                        return Err(ConsoleError::new("Try again"));
                    }
                    Ok(json!({"name":value.as_str().unwrap_or_default().to_uppercase()}))
                });
                let mut style = OutputStyle::new(&reading, output);
                let name = style
                    .ask_with_validator("Name", Some("Guest"), Some(validator))
                    .await?;
                let secret = style
                    .ask_hidden_with(
                        "Password",
                        Some(QuestionCallback::sync(|value| {
                            Ok(json!({"length":value.as_str().unwrap_or_default().len()}))
                        })),
                    )
                    .await?;
                let confirm = style.confirm("Continue").await?;
                let choice = style
                    .choice_with("Color", ["red", "green"], Some(json!("green")), false)
                    .await?;
                let meta = json!({"name":name,"secret":secret,"confirm":confirm,"choice":choice});
                output
                    .stderr
                    .push_str(&format!("STYLE_QUESTIONS_META:{meta}\n"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn expected_meta() -> Value {
    json!({"name":{"name":"ADA"},"secret":{"length":13},"confirm":true,"choice":"green"})
}
fn helper_routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [
        ("/style-questions", false),
        ("/style-questions/error", true),
    ] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args = vec!["--no-ansi".into()];
            if invalid {
                args.push("--invalid".into());
            }
            openrust::support::facades::Artisan::call("styles:questions", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            assert!(!output.stdout.contains("secret"));
            let meta: Value = serde_json::from_str(
                output
                    .stderr
                    .trim()
                    .strip_prefix("STYLE_QUESTIONS_META:")
                    .unwrap(),
            )
            .unwrap();
            openrust::http::Response::json(http::StatusCode::OK, meta)
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_style_helpers_and_runtime_failure_recovery() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing},
    };
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(
            Routing::default()
                .commands(register_style_question_helpers)
                .web(helper_routes),
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
    for (path, status) in [
        ("/style-questions", 200),
        ("/style-questions/error", 500),
        ("/style-questions", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(
            response.headers()[reqwest::header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let body: Value = response.json().await.unwrap();
        if status == 200 {
            assert_eq!(body, expected_meta());
        } else {
            assert!(body.is_object());
        }
    }
    let mut pending = Vec::new();
    for _ in 0..12 {
        pending.push(tokio::spawn(async move {
            let response = reqwest::get(format!("http://{address}/style-questions"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body, expected_meta());
        }));
    }
    for task in pending {
        task.await.unwrap();
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn real_cli_style_helpers_exit_zero_and_one() {
    let root = tempfile::tempdir().unwrap();
    for invalid in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "style_questions_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("STYLE_QUESTIONS_ROOT", root.path());
        if invalid {
            command.env("STYLE_QUESTIONS_INVALID", "1");
        }
        let child = command.output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(i32::from(invalid)),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let stderr = String::from_utf8(child.stderr).unwrap();
        assert!(!stdout.contains("secret"));
        if invalid {
            assert!(stderr.contains("validator runtime"));
        } else {
            let meta: Value =
                serde_json::from_str(stderr.trim().strip_prefix("STYLE_QUESTIONS_META:").unwrap())
                    .unwrap();
            assert_eq!(meta, expected_meta());
        }
    }
}
#[test]
#[ignore = "由真实HTTP/CLI/终端父消费者驱动"]
fn style_questions_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let app = Application::configure(std::env::var_os("STYLE_QUESTIONS_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_style_question_helpers))
        .create()
        .unwrap();
    let mut args = vec!["styles:questions".into(), "--no-ansi".into()];
    if std::env::var_os("STYLE_QUESTIONS_INVALID").is_some() {
        args.push("--invalid".into());
    }
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&args))
        .unwrap();
    if let Some(path) = std::env::var_os("STYLE_QUESTIONS_RELEASE") {
        eprintln!("STYLE_QUESTIONS_STATUS:{status}");
        let path = std::path::PathBuf::from(path);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    std::process::exit(status);
}
