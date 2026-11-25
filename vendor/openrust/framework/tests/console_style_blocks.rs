use openrust::console::{
    AsOutput, CommandDefinition, Output, OutputOptions, OutputReference, OutputStyle,
    OutputVerbosity, contracts::NewLineAware,
};
use serde_json::{Value, json};
#[derive(Clone)]
struct RepeatFormatter(openrust::console::OutputFormatter);
impl openrust::console::OutputFormatterInterface for RepeatFormatter {
    fn set_decorated(&self, value: bool) {
        self.0.set_decorated(value);
    }
    fn is_decorated(&self) -> bool {
        self.0.is_decorated()
    }
    fn set_style(&self, name: &str, style: openrust::console::FormatterStyle) {
        self.0.set_style(name, style);
    }
    fn has_style(&self, name: &str) -> bool {
        self.0.has_style(name)
    }
    fn get_style(
        &self,
        name: &str,
    ) -> Result<openrust::console::FormatterStyle, openrust::console::ConsoleFormatError> {
        self.0.get_style(name)
    }
    fn format(
        &self,
        message: Option<&str>,
    ) -> Result<Option<String>, openrust::console::ConsoleFormatError> {
        Ok(Some(self.0.format_optional(message)?.replace('X', "XX")))
    }
}
fn verbosity(value: &Value) -> OutputVerbosity {
    match value.as_u64().unwrap() {
        8 => OutputVerbosity::Silent,
        16 => OutputVerbosity::Quiet,
        32 => OutputVerbosity::Normal,
        _ => unreachable!(),
    }
}
fn parent_counts(output: &mut OutputReference<'_>) -> Vec<Value> {
    let mut counts = Vec::new();
    if let Some(view) = output.as_new_line_aware() {
        counts.push(json!([view.new_lines_written(), view.new_line_written()]));
        counts.extend(parent_counts(&mut output.get_output().unwrap()));
    }
    counts
}
fn run_case(style: &mut OutputStyle<'_>, case: &Value) -> Vec<Value> {
    let mut steps = Vec::new();
    for op in case["operations"].as_array().unwrap() {
        let value = &op[1];
        let items: Vec<&str> = if let Some(values) = value.as_array() {
            values.iter().map(|v| v.as_str().unwrap()).collect()
        } else {
            vec![value.as_str().unwrap_or("")]
        };
        let messages: openrust::console::OutputMessages<'_> = if value.is_array() {
            items.as_slice().into()
        } else {
            items[0].into()
        };
        let result = match op[0].as_str().unwrap() {
            "write" => style.write(items[0], false, OutputOptions::default()),
            "columns" => {
                unsafe {
                    std::env::set_var("COLUMNS", op[1].to_string());
                }
                Ok(())
            }
            "level" => {
                style.set_verbosity(verbosity(&op[1]));
                Ok(())
            }
            "block" => {
                let opts = &op[2];
                let mut options = openrust::console::BlockOptions::default();
                if let Some(value) = opts.get("kind") {
                    options.kind = value.as_str();
                }
                if let Some(value) = opts.get("style") {
                    options.style = value.as_str();
                }
                if let Some(value) = opts["prefix"].as_str() {
                    options.prefix = value;
                }
                if let Some(value) = opts["padding"].as_bool() {
                    options.padding = value;
                }
                if let Some(value) = opts["escape"].as_bool() {
                    options.escape = value;
                }
                style.block(messages, options)
            }
            "comment" => style.comment(messages),
            "success" => style.success(messages),
            "error" => style.error(messages),
            "warning" => style.warning(messages),
            "note" => style.note(messages),
            "info" => style.info(messages),
            "caution" => style.caution(messages),
            _ => unreachable!(),
        };
        assert_eq!(
            result.is_err(),
            case["steps"][steps.len()]["error"].is_string()
        );
        if let Err(error) = &result {
            use std::error::Error as _;
            if case["steps"][steps.len()]["error"] == "DivisionByZeroError" {
                assert!(error.to_string().contains("宽度为零"));
            } else {
                assert!(
                    error
                        .source()
                        .is_some_and(|source| source.is::<openrust::console::ConsoleFormatError>())
                );
            }
        }
        let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
        counts.extend(parent_counts(style.get_output()));
        counts.reverse();
        steps.push(json!({"output":style.get_output().stdout,"counts":counts,"error":case["steps"][steps.len()]["error"],"message":case["steps"][steps.len()]["message"]}));
    }
    steps
}
#[test]
fn real_source_styled_block_sequences() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "style_blocks_driver", "--ignored", "--nocapture"])
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
#[ignore = "由父消费者隔离终端尺寸"]
fn style_blocks_driver() {
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let input = CommandDefinition::from_signature("style-block")
        .unwrap()
        .parse(&[])
        .unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/style-block-oracle.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        unsafe {
            std::env::set_var("COLUMNS", case["width"].to_string());
        }
        let mut output = Output::default();
        output.set_verbosity(verbosity(&case["level"]));
        output.set_decorated(case["decorated"].as_bool().unwrap());
        if case["custom"] == true {
            output.set_formatter(std::sync::Arc::new(RepeatFormatter(
                openrust::console::OutputFormatter::new(case["decorated"].as_bool().unwrap()),
            )));
        }

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
            run_case(&mut second, case)
        } else {
            run_case(&mut first, case)
        };
        if actual != *case["steps"].as_array().unwrap() {
            failures.push(format!(
                "case {index}: actual {actual:?}, expected {}",
                case["steps"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn real_question_retry_uses_same_block_layout_and_captured_width() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "style_blocks_question_driver",
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
async fn question_case(style: &mut OutputStyle<'_>, case: &Value) -> Value {
    use openrust::console::{ConsoleError, Question, QuestionCallback};
    unsafe {
        std::env::set_var("COLUMNS", case["later_width"].to_string());
    }
    let mut question = Question::new("Name", Some(json!("Guest"))).unwrap();
    question.set_max_attempts(Some(2)).unwrap();
    question.set_validator(Some(QuestionCallback::sync(|value| {
        if value == "bad" {
            Err(ConsoleError::new("Incorrect X <info>value</info>\\"))
        } else {
            Ok(value)
        }
    })));
    let result = style.ask_question(&question).await;
    assert_eq!(result.is_err(), case["error"].is_string());
    let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
    counts.extend(parent_counts(style.get_output()));
    counts.reverse();
    json!({"value":result.ok(),"output":style.get_output().stdout,"counts":counts})
}
#[test]
#[ignore = "由父消费者隔离输入与终端尺寸"]
fn style_blocks_question_driver() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/style-block-question-oracle.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        unsafe {
            std::env::set_var("COLUMNS", case["width"].to_string());
        }
        let mut input = CommandDefinition::from_signature("block-question")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_interactive(true);
        input.set_stream(Some(openrust::console::ConsoleInputStream::new(
            tokio::io::BufReader::new(std::io::Cursor::new(b"bad\nAda\n".to_vec())),
        )));
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        if case["custom"] == true {
            output.set_formatter(std::sync::Arc::new(RepeatFormatter(
                openrust::console::OutputFormatter::new(case["decorated"].as_bool().unwrap()),
            )));
        }

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
            runtime.block_on(question_case(&mut second, case))
        } else {
            runtime.block_on(question_case(&mut first, case))
        };
        let expected =
            json!({"value":case["value"],"output":case["output"],"counts":case["counts"]});
        if actual != expected {
            failures.push(format!(
                "case {index}: actual {actual:?}, expected {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
fn register_style_blocks() -> Result<(), openrust::console::ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:block {--invalid}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut style = OutputStyle::new(input, output);
                if input.option("invalid").and_then(|v| v.as_bool()) == Some(true) {
                    return style
                        .block(
                            "bad",
                            openrust::console::BlockOptions {
                                prefix: "<fg=not-a-colour>!</>",
                                ..openrust::console::BlockOptions::default()
                            },
                        )
                        .map(|()| 0);
                }
                let messages = &["first <info>line</info>", "中文 👩‍🔬 and words"];
                style.comment(messages)?;
                style.success(messages)?;
                style.error(messages)?;
                style.warning(messages)?;
                style.note(messages)?;
                style.info(messages)?;
                style.caution(messages)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn expected(decorated: bool) -> String {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/style-block-oracle.json"
    ))
    .unwrap();
    let case = cases
        .iter()
        .find(|c| {
            c["level"] == 32
                && c["decorated"] == decorated
                && c["depth"] == 1
                && c["section"] == false
                && c["width"] == 80
                && c["operations"][0][0] == "comment"
                && c["operations"][0][1].is_array()
        })
        .unwrap();
    case["steps"].as_array().unwrap().last().unwrap()["output"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn block_routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/style-block", false), ("/style-block/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args = vec!["--no-ansi".into()];
            if invalid {
                args.push("--invalid".into());
            }
            let status =
                openrust::support::facades::Artisan::call("styles:block", &args, &mut output)
                    .await
                    .map_err(|source| openrust::http::HttpError::Internal {
                        source: Box::new(source),
                    })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                json!({"status":status,"output":output.stdout}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "由父消费者隔离终端尺寸"]
async fn real_http_block_failure_recovery_and_concurrent_isolation() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing},
    };
    use std::sync::Arc;
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
                .commands(register_style_blocks)
                .web(block_routes),
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
        ("/style-block", 200),
        ("/style-block/error", 500),
        ("/style-block", 200),
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
            assert_eq!(body, json!({"status":0,"output":expected(false)}));
        } else {
            assert!(body.is_object());
        }
    }
    let mut pending = Vec::new();
    for _ in 0..12 {
        pending.push(tokio::spawn(async move {
            let response = reqwest::get(format!("http://{address}/style-block"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body, json!({"status":0,"output":expected(false)}));
        }));
    }
    for task in pending {
        task.await.unwrap();
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn real_artisan_block_style_and_failure_exit() {
    let root = tempfile::tempdir().unwrap();
    for invalid in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "style_blocks_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("STYLE_BLOCK_ROOT", root.path())
            .env("COLUMNS", "80");
        if invalid {
            command.env("STYLE_BLOCK_INVALID", "1");
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
        if invalid {
            assert!(stderr.contains("not-a-colour"), "{stderr:?}");
        } else {
            assert!(stdout.contains(&expected(false)), "{stdout:?}");
        }
    }
}
#[test]
#[ignore = "由实际命令父消费者驱动"]
fn style_blocks_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let app = Application::configure(std::env::var_os("STYLE_BLOCK_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_style_blocks))
        .create()
        .unwrap();
    let mut args = vec!["styles:block".into(), "--no-ansi".into()];
    let invalid = std::env::var_os("STYLE_BLOCK_INVALID").is_some();
    if invalid {
        args.push("--invalid".into());
    }
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&args))
        .unwrap();
    assert_eq!(status, i32::from(invalid));
    std::process::exit(status);
}

#[test]
fn real_http_block_requests_run_with_fixed_terminal_width() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "real_http_block_failure_recovery_and_concurrent_isolation",
            "--ignored",
            "--nocapture",
        ])
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
