use openrust::console::{
    AsOutput, CommandDefinition, Output, OutputOptions, OutputReference, OutputStyle,
    OutputVerbosity, contracts::NewLineAware,
};
use serde_json::{Value, json};
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
        let result = match op[0].as_str().unwrap() {
            "title" => style.title(op[1].as_str().unwrap()),
            "section" => style.section(op[1].as_str().unwrap()),
            "write" => style.write(op[1].as_str().unwrap(), false, OutputOptions::default()),
            "level" => {
                style.set_verbosity(verbosity(&op[1]));
                Ok(())
            }
            "listing" => style.listing(
                op[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap()),
            ),
            "text" => {
                if let Some(items) = op[1].as_array() {
                    let messages: Vec<_> = items.iter().map(|v| v.as_str().unwrap()).collect();
                    style.text(messages.as_slice())
                } else {
                    style.text(op[1].as_str().unwrap())
                }
            }
            _ => unreachable!(),
        };
        assert_eq!(
            result.is_err(),
            case["steps"][steps.len()]["error"].is_string()
        );
        if let Err(error) = &result {
            use std::error::Error as _;
            assert!(
                error
                    .source()
                    .unwrap()
                    .is::<openrust::console::ConsoleFormatError>()
            );
        }
        let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
        counts.extend(parent_counts(style.get_output()));
        counts.reverse();
        steps.push(json!({"output":style.get_output().stdout,"counts":counts,"error":case["steps"][steps.len()]["error"]}));
    }
    steps
}
#[test]
fn real_source_headings_text_and_listing_sequences() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "style_text_driver", "--ignored", "--nocapture"])
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
fn style_text_driver() {
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let input = CommandDefinition::from_signature("style-text")
        .unwrap()
        .parse(&[])
        .unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/style-text-oracle.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let mut output = Output::default();
        output.set_verbosity(verbosity(&case["level"]));
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

fn register_style_text() -> Result<(), openrust::console::ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:text {--invalid}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut style = OutputStyle::new(input, output);
                if input.option("invalid").and_then(|v| v.as_bool()) == Some(true) {
                    return style.title("<fg=not-a-colour>bad</>").map(|()| 0);
                }
                style.title("Hello")?;
                style.section("Part")?;
                style.text(&["one", "two"])?;
                style.listing(["A", "B"])?;
                style.title("")?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn expected(decorated: bool) -> String {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/style-text-oracle.json"
    ))
    .unwrap();
    let case = cases
        .iter()
        .find(|c| {
            c["level"] == 32
                && c["decorated"] == decorated
                && c["depth"] == 1
                && c["section"] == false
                && c["operations"][0][1] == "Hello"
        })
        .unwrap();
    case["steps"].as_array().unwrap().last().unwrap()["output"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn text_routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/style-text", false), ("/style-text/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args = vec!["--no-ansi".into()];
            if invalid {
                args.push("--invalid".into());
            }
            let status =
                openrust::support::facades::Artisan::call("styles:text", &args, &mut output)
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
async fn real_http_text_failure_recovery_and_concurrent_isolation() {
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
                .commands(register_style_text)
                .web(text_routes),
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
        ("/style-text", 200),
        ("/style-text/error", 500),
        ("/style-text", 200),
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
            let response = reqwest::get(format!("http://{address}/style-text"))
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
fn real_artisan_text_style_and_failure_exit() {
    let root = tempfile::tempdir().unwrap();
    for invalid in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "style_text_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("STYLE_TEXT_ROOT", root.path());
        if invalid {
            command.env("STYLE_TEXT_INVALID", "1");
        }
        let child = command.output().unwrap();
        assert!(
            child.status.success(),
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
fn style_text_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let app = Application::configure(std::env::var_os("STYLE_TEXT_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_style_text))
        .create()
        .unwrap();
    let mut args = vec!["styles:text".into(), "--no-ansi".into()];
    let invalid = std::env::var_os("STYLE_TEXT_INVALID").is_some();
    if invalid {
        args.push("--invalid".into());
    }
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&args))
        .unwrap();
    assert_eq!(status, i32::from(invalid));
}
