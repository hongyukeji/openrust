use openrust::console::{
    AsOutput, CommandDefinition, ConsoleInputStream, Output, OutputFormat, OutputOptions,
    OutputReference, OutputStyle, OutputVerbosity, Question, contracts::NewLineAware,
};
use serde_json::{Value, json};
fn level(value: &Value) -> OutputVerbosity {
    match value.as_u64().unwrap() {
        8 => OutputVerbosity::Silent,
        16 => OutputVerbosity::Quiet,
        32 => OutputVerbosity::Normal,
        64 => OutputVerbosity::Verbose,
        256 => OutputVerbosity::Debug,
        _ => panic!("未知级别"),
    }
}
fn options(value: &Value) -> OutputOptions {
    match value.as_u64().unwrap() {
        2 => OutputFormat::Raw.into(),
        4 => OutputFormat::Plain.into(),
        64 => OutputVerbosity::Verbose.into(),
        0 => OutputOptions::default(),
        _ => panic!("未知选项"),
    }
}
fn messages(value: &Value) -> Vec<&str> {
    match value {
        Value::Array(values) => values.iter().map(|v| v.as_str().unwrap()).collect(),
        _ => vec![value.as_str().unwrap()],
    }
}
fn counts(output: &mut OutputReference<'_>) -> Vec<Value> {
    let mut result = Vec::new();
    if let Some(view) = output.as_new_line_aware() {
        result.push(json!([view.new_lines_written(), view.new_line_written()]));
        result.extend(counts(&mut output.get_output().unwrap()));
    }
    result
}
fn bottom_write(output: &mut OutputReference<'_>, text: &str, raw_root: bool) {
    if let Some(mut inner) = output.get_output() {
        if raw_root || inner.as_new_line_aware().is_some() {
            bottom_write(&mut inner, text, raw_root);
        } else {
            output.write(text, false, OutputOptions::default()).unwrap();
        }
    } else {
        output.write(text, false, OutputOptions::default()).unwrap();
    }
}
async fn run_case(case: &Value, style: &mut OutputStyle<'_>) -> Vec<Value> {
    let mut actual = Vec::new();
    for op in case["operations"].as_array().unwrap() {
        let mut value = Value::Null;
        match op[0].as_str().unwrap() {
            "write" if op[1] == 0 => {
                bottom_write(&mut style.as_output(), op[2].as_str().unwrap(), false)
            }
            "write" => {
                let items = messages(&op[2]);
                if op[2].is_array() {
                    style
                        .write(items.as_slice(), op[3].as_bool().unwrap(), options(&op[4]))
                        .unwrap();
                } else {
                    style
                        .write(items[0], op[3].as_bool().unwrap(), options(&op[4]))
                        .unwrap();
                }
            }
            "writeln" => {
                let items = messages(&op[2]);
                if op[2].is_array() {
                    style.writeln(items.as_slice(), options(&op[3])).unwrap();
                } else {
                    style.writeln(items[0], options(&op[3])).unwrap();
                }
            }
            "newLine" => style.new_line(op[2].as_u64().unwrap() as usize).unwrap(),
            "level" => style.set_verbosity(level(&op[2])),
            "root" => bottom_write(style.get_output(), op[1].as_str().unwrap(), true),
            "ask" => {
                value = style
                    .ask_question(&Question::new("Name", Some(json!("Guest"))).unwrap())
                    .await
                    .unwrap();
            }
            other => panic!("未知操作 {other}"),
        }
        let mut state = vec![json!([style.new_lines_written(), style.new_line_written()])];
        state.extend(counts(style.get_output()));
        state.reverse();
        actual.push(
            json!({"output":style.get_output().stdout,"counts":state,"error":null,"value":value}),
        );
    }
    actual
}
#[test]
fn nested_source_sequences_in_isolated_process() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "nested_style_source_driver",
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
#[ignore = "由父消费者在独立进程中驱动"]
fn nested_style_source_driver() {
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/nested-style-oracle.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let mut input = CommandDefinition::from_signature("nested")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_interactive(true);
        let answer = case["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v[0] == "ask")
            .map_or("", |v| v[2].as_str().unwrap());
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            std::io::Cursor::new(answer.as_bytes().to_vec()),
        ))));
        let mut output = Output::default();
        output.set_verbosity(level(&case["level"]));
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let mut section;
        let target: &mut dyn AsOutput = if case["section"] == true {
            section = output.section();
            &mut section
        } else {
            &mut output
        };
        let mut first = OutputStyle::new(&input, target);
        let actual = match case["depth"].as_u64().unwrap() {
            1 => runtime.block_on(run_case(case, &mut first)),
            2 => {
                let mut second = OutputStyle::new(&input, &mut first);
                runtime.block_on(run_case(case, &mut second))
            }
            3 => {
                let mut second = OutputStyle::new(&input, &mut first);
                let mut third = OutputStyle::new(&input, &mut second);
                runtime.block_on(run_case(case, &mut third))
            }
            _ => unreachable!(),
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
fn trait_and_underlying_output_are_real_borrowed_consumers() {
    fn status(output: &impl NewLineAware) -> (usize, bool) {
        (output.new_lines_written(), output.new_line_written())
    }
    let input = CommandDefinition::from_signature("nested")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    assert!(output.as_output().as_new_line_aware().is_none());
    let mut first = OutputStyle::new(&input, &mut output);
    first.new_line(2).unwrap();
    assert_eq!(status(&first), (3, true));
    let mut second = OutputStyle::new(&input, &mut first);
    assert_eq!(status(&second), (3, true));
    second
        .get_output()
        .write("x", false, OutputOptions::default())
        .unwrap();
    assert_eq!(status(&second), (0, false));
    assert_eq!(
        status(&second.get_output().as_new_line_aware().unwrap()),
        (0, false)
    );
    second.writeln("y", OutputOptions::default()).unwrap();
    assert_eq!(status(&first), (1, true));
    assert!(output.stdout.ends_with("xy\n"));
}

fn progress_case(case: &Value, style: &mut OutputStyle<'_>) {
    let mut bar = openrust::console::ProgressBar::new(style, 3).unwrap();
    bar.set_format(case["format"].as_str().unwrap());
    bar.min_seconds_between_redraws(0.0)
        .max_seconds_between_redraws(0.0)
        .set_redraw_frequency(Some(1));
    bar.start().unwrap();
    bar.advance().unwrap();
    bar.get_output()
        .writeln("notice", OutputOptions::default())
        .unwrap();
    bar.advance().unwrap();
    bar.clear().unwrap();
    bar.display().unwrap();
    bar.finish().unwrap();
}
#[test]
fn wrapped_progress_and_section_identity_match_source() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "nested_progress_source_driver",
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
#[ignore = "由父消费者隔离终端尺寸"]
fn nested_progress_source_driver() {
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let input = CommandDefinition::from_signature("nested")
        .unwrap()
        .parse(&[])
        .unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/nested-progress-oracle.json"
    ))
    .unwrap();
    for (index, case) in cases.iter().enumerate() {
        let mut output = Output::default();
        output.set_verbosity(level(&case["level"]));
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let mut section;
        let target: &mut dyn AsOutput = if case["section"] == true {
            section = output.section();
            &mut section
        } else {
            &mut output
        };
        let mut first = OutputStyle::new(&input, target);
        let style = if case["depth"] == 2 {
            None
        } else {
            Some(&mut first)
        };
        let mut second;
        let style = if let Some(style) = style {
            style
        } else {
            second = OutputStyle::new(&input, &mut first);
            &mut second
        };
        progress_case(case, style);
        let mut state = vec![json!([style.new_lines_written(), style.new_line_written()])];
        state.extend(counts(style.get_output()));
        state.reverse();
        assert_eq!(
            style.get_output().stdout,
            case["output"].as_str().unwrap(),
            "progress {index}"
        );
        assert_eq!(state, *case["counts"].as_array().unwrap(), "counts {index}");
        assert!(case["append_error"].as_str().is_some());
        let mut outer = OutputStyle::new(&input, style);
        assert!(outer.get_output().as_section().is_err());
        let mut table = openrust::console::Table::new(&mut outer).unwrap();
        table.set_rows([vec!["x"]]);
        assert!(table.append_row(vec!["y"]).is_err());
    }
}
fn register_nested_command() -> Result<(), openrust::console::ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:nested {--invalid} {--progress}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut first = OutputStyle::new(input, output);
                if input.option("progress").and_then(|value| value.as_bool()) == Some(true) {
                    let mut bar = openrust::console::ProgressBar::new(&mut first, 1)?;
                    bar.set_format("nested:%current%/%max%");
                    bar.start()?;
                    bar.finish()?;
                    return Ok(0);
                }

                first.new_line(1)?;
                {
                    let mut second = OutputStyle::new(input, &mut first);
                    second.write("A", false, OutputOptions::default())?;
                    second.new_line(2)?;
                    assert_eq!(
                        second
                            .get_output()
                            .as_new_line_aware()
                            .unwrap()
                            .new_lines_written(),
                        2
                    );
                    if input.option("invalid").and_then(|value| value.as_bool()) == Some(true) {
                        let mut table = openrust::console::Table::new(&mut second)?;
                        table.append_row(vec!["invalid"])?;
                    }
                    second.writeln("B", OutputOptions::default())?;
                }
                assert_eq!(first.new_lines_written(), 1);
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn nested_routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/nested", false), ("/nested/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let args = if invalid {
                vec!["--invalid".into()]
            } else {
                vec![]
            };
            let status =
                openrust::support::facades::Artisan::call("styles:nested", &args, &mut output)
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
async fn real_http_nested_command_failure_and_recovery() {
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
                .commands(register_nested_command)
                .web(nested_routes),
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
    for (path, status) in [("/nested", 200), ("/nested/error", 500), ("/nested", 200)] {
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
            assert_eq!(body, json!({"status":0,"output":"\nA\n\nB\n"}));
        } else {
            assert!(body.is_object());
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn real_artisan_nested_style_output_and_exit() {
    let root = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "nested_cli_driver", "--ignored", "--nocapture"])
        .env("NESTED_STYLE_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let stdout = String::from_utf8(child.stdout).unwrap();
    assert!(stdout.contains("\nA\n\nB\n"), "{stdout:?}");
}
#[test]
#[ignore = "由实际命令父消费者驱动"]
fn nested_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let app = Application::configure(std::env::var_os("NESTED_STYLE_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_nested_command))
        .create()
        .unwrap();
    let mut args = vec!["styles:nested".into(), "--no-ansi".into()];
    if std::env::var_os("NESTED_STYLE_PROGRESS").is_some() {
        args.push("--progress".into());
    }
    let status = runtime.block_on(app.handle_command(&args)).unwrap();
    assert_eq!(status, 0);
}

#[test]
fn nested_progress_uses_selected_stdout_in_real_artisan() {
    let root = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "nested_cli_driver", "--ignored", "--nocapture"])
        .env("NESTED_STYLE_ROOT", root.path())
        .env("NESTED_STYLE_PROGRESS", "1")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let stdout = String::from_utf8(child.stdout).unwrap();
    assert!(
        stdout.contains("nested:0/1") && stdout.contains("nested:1/1"),
        "{stdout:?}"
    );
    assert!(child.stderr.is_empty(), "{:?}", child.stderr);
}
