use openrust::{
    console::{
        AsOutput, AsyncCommandCallback, ConsoleError, Input, Output, OutputFormat, OutputReference,
        OutputVerbosity,
    },
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "nested:child {label} {--error}",
        |input: &Input, output: &mut Output| {
            let mut io = input.io(output);
            if input.option("error") == Some(json!(true)) {
                return Err(ConsoleError::new("nested child error"));
            }
            let status = json!({
                "label":input.argument("label"),
                "interactive":input.is_interactive(),
                "verbose":input.option("verbose"),
                "quiet":input.option("quiet"),
                "ansi":input.option("ansi"),
                "no_ansi":input.option("no-ansi"),
            });
            io.line_with(&status.to_string(), None, Some(OutputVerbosity::Quiet))?;
            Ok(7)
        },
    )?;
    Artisan::command_with(
        "nested:parent {mode}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut io = input.io(output);
                io.line_with("parent begin", None, Some(OutputVerbosity::Quiet))?;
                let mode = io.argument("mode").unwrap();
                let code = match mode.as_str() {
                    Some("silent") => {
                        io.call_silently("nested:child", &["Ada Lovelace".into()])
                            .await?
                    }
                    Some("interactive") => {
                        io.call_named(
                            "nested:child",
                            &[
                                ("label", json!("Ada Lovelace")),
                                ("--no-interaction", json!(false)),
                            ],
                        )
                        .await?
                    }
                    Some("failure") => {
                        io.call("nested:child", &["Ada Lovelace".into(), "--error".into()])
                            .await?
                    }
                    _ => io.call("nested:child", &["Ada Lovelace".into()]).await?,
                };
                io.line_with(
                    &format!("parent after={code}"),
                    None,
                    Some(OutputVerbosity::Quiet),
                )?;
                Ok(code)
            })
        }),
    )?;
    Artisan::command_with(
        "nested:bytes {--fail}",
        |input: &Input, output: &mut Output| {
            output.stdout.push_str("child stdout\n");
            output.stderr.push_str("child stderr\n");
            if input.option("fail") == Some(json!(true)) {
                return Err(ConsoleError::new("child stopped"));
            }
            Ok(3)
        },
    )?;
    Ok(())
}

fn application(root: &std::path::Path) -> Application {
    Application::configure(root.into())
        .with_routing(Routing::default().commands(register).web(routes))
        .create()
        .unwrap()
}

#[tokio::test]
async fn native_nested_options_output_and_results_follow_command_source() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/artisan-nested-call-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = application(root.path());
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for row in rows {
            let mut output = Output::default();
            let mut args = vec![row["mode"].as_str().unwrap().into()];
            if let Some(flags) = row["flags"].as_object() {
                args.extend(flags.keys().map(std::ffi::OsString::from));
            }
            let result = Artisan::call("nested:parent", &args, &mut output).await;
            assert_eq!(
                result.as_ref().ok().copied(),
                row["status"].as_i64().map(|code| code as i32),
                "{row}: {result:?}"
            );
            if let Err(error) = result {
                assert!(error.to_string().contains("nested child error"));
            }
            let mut expected = row["out"].as_str().unwrap().lines();
            let mut actual = output.stdout.lines();
            assert_eq!(actual.next(), expected.next(), "{row}");
            let expected_next = expected.next();
            if let Some(value) = expected_next.filter(|line| line.starts_with('{')) {
                let mut wanted: Value = serde_json::from_str(value).unwrap();
                let actual_value: Value = serde_json::from_str(actual.next().unwrap()).unwrap();
                // 固定 PHP 否定颜色开关无法出现在选项合并表；原生保留显式负值。
                if row["flags"]["--no-ansi"] == true {
                    wanted["ansi"] = json!(false);
                }
                if row["flags"]["--ansi"] == true {
                    wanted["no_ansi"] = Value::Null;
                }
                // 安静输出沿用当前控制台禁止交互管线。
                if row["flags"]["--quiet"] == true {
                    wanted["interactive"] = json!(false);
                }
                assert_eq!(actual_value, wanted, "{row}");
                assert_eq!(actual.next(), expected.next());
            } else {
                assert_eq!(actual.next(), expected_next, "{row}");
            }
            assert!(actual.next().is_none());
        }
    })
    .await;
}

struct CustomOutput {
    output: Output,
    borrows: usize,
}
impl AsOutput for CustomOutput {
    fn as_output(&mut self) -> OutputReference<'_> {
        self.borrows += 1;
        self.output.as_output()
    }
}

#[tokio::test]
async fn programmatic_calls_use_custom_output_and_selected_console_section() {
    let root = tempfile::tempdir().unwrap();
    let mut app = application(root.path());
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let (code, captured) = Artisan::call_captured("nested:bytes", &[]).await.unwrap();
        assert_eq!(code, 3);
        assert_eq!(captured.stdout, "child stdout\n");
        assert_eq!(captured.stderr, "child stderr\n");
        let mut custom = CustomOutput {
            output: Output::default(),
            borrows: 0,
        };
        custom.output.stdout.push_str("existing\n");
        let code = Artisan::call("nested:bytes", &[], &mut custom)
            .await
            .unwrap();
        assert_eq!(code, 3);
        assert_eq!(custom.borrows, 1);
        assert_eq!(custom.output.stdout, "existing\nchild stdout\n");
        assert_eq!(custom.output.stderr, "child stderr\n");

        let mut root = Output::default();
        root.set_decorated(true);
        let mut section = root.section();
        section.writeln("section begin", OutputFormat::Raw).unwrap();
        let error = Artisan::call("nested:bytes", &["--fail".into()], &mut section)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("child stopped"));
        assert_eq!(
            section.get_content(),
            "section begin\nchild stdout\nchild stderr\n"
        );
        let code = Artisan::call_line("nested:bytes", &mut section)
            .await
            .unwrap();
        assert_eq!(code, 3);
        assert_eq!(
            section.get_content(),
            "section begin\nchild stdout\nchild stderr\nchild stdout\nchild stderr\n"
        );
        assert!(root.stderr.is_empty());
    })
    .await;
}

#[tokio::test]
async fn default_custom_and_physical_section_outputs_match_fixed_consumers() {
    fn lines(value: &str) -> Vec<Value> {
        value
            .lines()
            .map(|line| serde_json::from_str(line).unwrap_or_else(|_| json!(line)))
            .collect()
    }

    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/artisan-nested-output-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = application(root.path());
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let args = ["plain".into(), "--no-interaction".into()];
        for case in cases {
            let (status, text) = match case["kind"].as_str().unwrap() {
                "default" => {
                    let (status, output) = Artisan::call_captured("nested:parent", &args)
                        .await
                        .unwrap();
                    (status, output.stdout)
                }
                "custom" => {
                    let mut target = CustomOutput {
                        output: Output::default(),
                        borrows: 0,
                    };
                    target.output.stdout.push_str("custom prefix\n");
                    let status = Artisan::call("nested:parent", &args, &mut target)
                        .await
                        .unwrap();
                    assert_eq!(target.borrows, 1);
                    (status, target.output.stdout)
                }
                _ => {
                    let mut output = Output::default();
                    output.set_decorated(case["decorated"] == true);
                    let mut section = output.section();
                    section
                        .writeln("section prefix", OutputFormat::Raw)
                        .unwrap();
                    let status = Artisan::call("nested:parent", &args, &mut section)
                        .await
                        .unwrap();
                    assert_eq!(
                        lines(&section.get_content()),
                        lines(case["contents"].as_str().unwrap())
                    );
                    (status, output.stdout)
                }
            };
            assert_eq!(status, case["code"]);
            let expected = case["out"]
                .as_str()
                .or_else(|| case["stream"].as_str())
                .unwrap();
            assert_eq!(lines(&text), lines(expected));
        }
    })
    .await;
}

#[test]
#[ignore = "由父消费者执行实际 Artisan"]
fn nested_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = application(root.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mode = std::env::var("NESTED_COMMAND_MODE").unwrap();
    let arguments = [
        "nested:parent".into(),
        mode.into(),
        "--no-interaction".into(),
        "--no-ansi".into(),
    ];
    let status = runtime.block_on(app.handle_command(&arguments)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(status);
}

#[test]
fn real_artisan_nested_helpers_share_parent_stream_and_can_suppress_child() {
    for (mode, code, child_is_visible) in [
        ("plain", 7, true),
        ("silent", 7, false),
        ("failure", 1, false),
    ] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "nested_cli_driver", "--ignored", "--nocapture"])
            .env("NESTED_COMMAND_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("parent begin"));
        assert_eq!(stdout.contains("Ada Lovelace"), child_is_visible);
        if code == 7 {
            assert!(stdout.contains("parent after=7"), "{stdout}");
        } else {
            assert!(String::from_utf8_lossy(&result.stderr).contains("nested child error"));
        }
    }
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/nested", || async {
        let mut output = Output::default();
        output.set_decorated(true);
        let mut section = output.section();
        let code = Artisan::call(
            "nested:parent",
            &["plain".into(), "--no-interaction".into()],
            &mut section,
        )
        .await
        .map_err(|error| openrust::http::HttpError::Internal {
            source: Box::new(error),
        })?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"status":code,"output":section.get_content()}),
        )
    })?;
    Ok(())
}

#[tokio::test]
async fn real_http_nested_output_reaches_selected_section_without_losing_exit() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(application(root.path()));
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
    let response = reqwest::get(format!("http://{address}/nested"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], 7);
    let result = body["output"].as_str().unwrap();
    assert!(result.contains("parent begin"));
    assert!(result.contains("Ada Lovelace"));
    assert!(result.contains("parent after=7"));
    server.abort();
    let _ = server.await;
}
