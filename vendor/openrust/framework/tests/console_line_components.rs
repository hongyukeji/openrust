use openrust::console::{
    AsOutput, CommandDefinition, ConsoleError, ConsoleStyle, Output, OutputOptions,
    OutputReference, OutputStyle, OutputVerbosity,
    view::components::{Error as ErrorComponent, Info, Line, Success, Warn},
};
use serde_json::{Value, json};
fn verbosity(value: u64) -> OutputVerbosity {
    match value {
        8 => OutputVerbosity::Silent,
        16 => OutputVerbosity::Quiet,
        64 => OutputVerbosity::Verbose,
        _ => OutputVerbosity::Normal,
    }
}
fn render(output: &mut (impl AsOutput + ?Sized), case: &Value) -> Result<(), ConsoleError> {
    let text = case["text"].as_str().unwrap();
    let required = case["required"].as_u64().unwrap();
    let options = OutputOptions {
        format: match required & 7 {
            1 => openrust::console::OutputFormat::ExplicitNormal,
            2 => openrust::console::OutputFormat::Raw,
            4 => openrust::console::OutputFormat::Plain,
            _ => openrust::console::OutputFormat::Normal,
        },
        verbosity: Some(verbosity(required & !7)),
    };
    macro_rules! component {
        ($name:ident) => {{
            let mut component = $name::new(output);
            if required == 32 {
                component.render(text)
            } else {
                component.render_with(text, options)
            }
        }};
    }
    match case["component"].as_str().unwrap() {
        "Info" => component!(Info),
        "Success" => component!(Success),
        "Warn" => component!(Warn),
        "Error" => component!(ErrorComponent),
        "Line" => {
            let mut component = Line::new(output);
            if required == 32 {
                component.render(case["style"].as_str().unwrap(), text)
            } else {
                component.render_with(case["style"].as_str().unwrap(), text, options)
            }
        }
        _ => unreachable!(),
    }
}
#[test]
fn all_five_public_types_match_actual_source_bytes_and_newline_state() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/line-components-oracle.json"
    ))
    .unwrap();
    for case in cases {
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        output.set_verbosity(verbosity(case["level"].as_u64().unwrap()));
        let input = CommandDefinition::from_signature("line:show")
            .unwrap()
            .parse(&[])
            .unwrap();
        let prefix = case["prefix"].as_str().unwrap();
        let lines = match case["surface"].as_str().unwrap() {
            "derived" => {
                let mut style = OutputStyle::new(&input, &mut output);
                if !prefix.is_empty() {
                    style
                        .write(prefix, false, OutputOptions::default())
                        .unwrap();
                }
                render(&mut style, &case).unwrap();
                json!(style.new_lines_written())
            }
            "nested" => {
                let mut parent = OutputStyle::new(&input, &mut output);
                let mut style = OutputStyle::new(&input, &mut parent);
                if !prefix.is_empty() {
                    style
                        .write(prefix, false, OutputOptions::default())
                        .unwrap();
                }
                render(&mut style, &case).unwrap();
                json!(style.new_lines_written())
            }
            "base" => {
                let mut style = ConsoleStyle::new(&input, &mut output);
                if !prefix.is_empty() {
                    style
                        .write(prefix, false, OutputOptions::default())
                        .unwrap();
                }
                render(&mut style, &case).unwrap();
                Value::Null
            }
            "raw" => {
                if !prefix.is_empty() {
                    OutputReference::from(&mut output)
                        .write(prefix, false, OutputOptions::default())
                        .unwrap();
                }
                render(&mut output, &case).unwrap();
                Value::Null
            }
            _ => unreachable!(),
        };
        assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
        assert_eq!(lines, case["new_lines"], "{case}");
        assert!(output.stderr.is_empty());
    }
}
#[test]
fn verbosity_is_explicit_and_unknown_styles_leave_output_unchanged() {
    assert_eq!(openrust::console::OutputFormat::Normal as u16, 0);
    assert_eq!(openrust::console::OutputFormat::ExplicitNormal as u16, 1);
    assert_eq!(openrust::console::OutputFormat::Raw as u16, 2);
    assert_eq!(openrust::console::OutputFormat::Plain as u16, 4);
    let mut output = Output::default();
    output.set_verbosity(OutputVerbosity::Quiet);
    Info::new(&mut output).render("hidden").unwrap();
    assert!(output.stdout.is_empty());
    Info::new(&mut output)
        .render_with("shown", OutputVerbosity::Quiet)
        .unwrap();
    Success::new(&mut output)
        .render_with("ok", OutputVerbosity::Quiet)
        .unwrap();
    Warn::new(&mut output)
        .render_with("caution", OutputVerbosity::Quiet)
        .unwrap();
    ErrorComponent::new(&mut output)
        .render_with("failure", OutputVerbosity::Quiet)
        .unwrap();
    let before = output.stdout.clone();
    assert!(Line::new(&mut output).render("missing", "text").is_err());
    assert_eq!(output.stdout, before);
    assert!(
        before.contains(" INFO ")
            && before.contains(" SUCCESS ")
            && before.contains(" WARN ")
            && before.contains(" ERROR ")
    );
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "line:show {--invalid}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let case = std::env::var("LINE_CASE").ok().map(|c| serde_json::from_str::<Value>(&c).unwrap()).unwrap_or_else(|| json!({"component":"Info","side":"stdout","text":"Created [report]","style":"info","required":32}));
                let mut style = OutputStyle::new(input, output);
                if input.option("invalid").and_then(|value| value.as_bool()) == Some(true) {
                    Line::new(&mut style).render("missing", "failure")?;
                } else if case["side"] == "stderr" {
                    render(&mut style.get_error_style(), &case)?;
                } else {
                    render(&mut style, &case)?;
                }
                if std::env::var_os("LINE_CASE").is_some() {
                    eprintln!("LINE_META{}", style.new_lines_written());
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父测试运行真实Artisan进程并核对stdout/stderr"]
fn line_component_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let case: Value = serde_json::from_str(&std::env::var("LINE_CASE").unwrap()).unwrap();
    let mut args = vec![
        "line:show".into(),
        "--no-interaction".into(),
        if case["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    if case["level"] == 16 {
        args.push("--quiet".into());
    }
    if case["invalid"] == true {
        args.push("--invalid".into());
    }
    print!("LINE_BEGIN");
    std::io::stdout().flush().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn real_artisan_console_streams_and_exit_codes_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/line-console-oracle.json"
    ))
    .unwrap();
    for case in cases {
        let mut input = case.clone();
        input["text"] = json!("Created [report]");
        input["required"] = json!(32);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "line_component_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("LINE_CASE", input.to_string())
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "{case}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert_eq!(
            stdout.split_once("LINE_BEGIN").unwrap().1,
            case["stdout"].as_str().unwrap(),
            "{case}"
        );
        let stderr = String::from_utf8(result.stderr).unwrap();
        let (actual, meta) = stderr.rsplit_once("LINE_META").unwrap();
        assert_eq!(actual, case["stderr"].as_str().unwrap(), "{case}");
        assert_eq!(
            meta.trim().parse::<usize>().unwrap(),
            case["new_lines"].as_u64().unwrap() as usize,
            "{case}"
        );
    }
    let case =
        json!({"component":"Info","text":"failure","style":"info","required":32,"invalid":true});
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "line_component_cli_driver",
            "--ignored",
            "--nocapture",
        ])
        .env("LINE_CASE", case.to_string())
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/line", false), ("/line/failure", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let args: Vec<std::ffi::OsString> = if invalid {
                vec!["--invalid".into()]
            } else {
                vec![]
            };
            openrust::support::facades::Artisan::call("line:show", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_uses_public_components_and_recovers_after_render_failure() {
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
        .with_routing(Routing::default().web(routes).commands(commands))
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
    for (path, status) in [("/line", 200), ("/line/failure", 500), ("/line", 200)] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"output":"\n   INFO  Created [report].  \n\n"})
            );
        }
    }
    let mut tasks = vec![];
    for _ in 0..12 {
        tasks.push(tokio::spawn(async move {
            let response = reqwest::get(format!("http://{address}/line"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"output":"\n   INFO  Created [report].  \n\n"})
            );
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn custom_formatter_style_binding_is_preserved_by_component_output() {
    use std::sync::Arc;
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/line-custom-formatter-oracle.json"
    ))
    .unwrap();
    let mut output = Output::default();
    output.set_decorated(true);
    let input = CommandDefinition::from_signature("custom")
        .unwrap()
        .parse(&[])
        .unwrap();
    let lines = {
        let mut style = OutputStyle::new(&input, &mut output);
        let formatter = style.get_formatter();
        let mut component = Info::new(&mut style);
        formatter.set_style(
            "bg=blue;fg=white",
            Arc::new(
                openrust::console::OutputFormatterStyle::new(
                    Some("magenta"),
                    None,
                    &["underscore"],
                )
                .unwrap(),
            ),
        );
        component.render("Custom [report]").unwrap();
        style.new_lines_written()
    };
    assert_eq!(json!({"output":output.stdout,"new_lines":lines}), expected);
}

#[test]
fn source_invalid_style_preserves_partial_output_and_newline_metadata() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/line-invalid-style-oracle.json"
    ))
    .unwrap();
    for case in cases {
        assert_eq!(case["error"][0], "TypeError");
        let mut output = Output::default();
        let input = CommandDefinition::from_signature("invalid")
            .unwrap()
            .parse(&[])
            .unwrap();
        let count = {
            let mut style = OutputStyle::new(&input, &mut output);
            if !case["prefix"].as_str().unwrap().is_empty() {
                style
                    .write(
                        case["prefix"].as_str().unwrap(),
                        false,
                        OutputOptions::default(),
                    )
                    .unwrap();
            }
            let error = Line::new(&mut style).render("missing", "text").unwrap_err();
            assert!(error.to_string().contains("missing"));
            style.new_lines_written()
        };
        assert_eq!(output.stdout, case["output"].as_str().unwrap());
        assert_eq!(json!(count), case["new_lines"]);
    }
}
