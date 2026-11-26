use openrust::{
    console::{ConsoleError, ConsoleTableError, Input, Output, Table, TableStyle},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
#[derive(Clone)]
struct PlainFormatter(openrust::console::OutputFormatter);
impl openrust::console::OutputFormatterInterface for PlainFormatter {
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
        Ok(Some(self.0.format_optional(message)?))
    }
}
#[derive(Clone)]
struct TrackingFormatter {
    base: PlainFormatter,
    calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    answer: String,
}
impl openrust::console::OutputFormatterInterface for TrackingFormatter {
    fn set_decorated(&self, value: bool) {
        self.base.0.set_decorated(value);
    }
    fn is_decorated(&self) -> bool {
        self.base.0.is_decorated()
    }
    fn set_style(&self, name: &str, style: openrust::console::FormatterStyle) {
        self.base.0.set_style(name, style);
    }
    fn has_style(&self, name: &str) -> bool {
        self.base.0.has_style(name)
    }
    fn get_style(
        &self,
        name: &str,
    ) -> Result<openrust::console::FormatterStyle, openrust::console::ConsoleFormatError> {
        self.base.0.get_style(name)
    }
    fn format(
        &self,
        message: Option<&str>,
    ) -> Result<Option<String>, openrust::console::ConsoleFormatError> {
        Ok(Some(self.base.0.format_optional(message)?))
    }
    fn as_wrappable(&self) -> Option<&dyn openrust::console::WrappableOutputFormatterInterface> {
        Some(self)
    }
}
impl openrust::console::WrappableOutputFormatterInterface for TrackingFormatter {
    fn format_and_wrap(
        &self,
        message: Option<&str>,
        width: i64,
    ) -> Result<String, openrust::console::ConsoleFormatError> {
        self.calls
            .lock()
            .unwrap()
            .push(serde_json::json!([message, width]));
        Ok(self.answer.clone())
    }
}
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/table-layout-oracle.json"
    ))
    .unwrap()
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:layout {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let cfg = &case["config"];
            if case["non_wrappable"] == true {
                output.set_formatter(std::sync::Arc::new(PlainFormatter(
                    openrust::console::OutputFormatter::new(case["decorated"].as_bool().unwrap()),
                )));
            }
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output()).unwrap();
            let rows: Vec<Vec<Value>> = case["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row.as_array().unwrap().clone())
                .collect();
            table
                .set_headers(case["headers"].as_array().unwrap().clone())
                .set_rows(rows)
                .set_style(TableStyle::from_name(case["style"].as_str().unwrap())?);
            if let Some(widths) = cfg["minimum"].as_array() {
                for pair in widths {
                    table.set_column_width(
                        pair[0].as_u64().unwrap() as usize,
                        pair[1].as_i64().unwrap(),
                    );
                }
            }
            if let Some(widths) = cfg["widths"].as_array() {
                table.set_column_widths(widths.iter().map(|v| v.as_i64().unwrap()));
            }
            if let Some(widths) = cfg["widths"].as_object() {
                table.set_column_widths_with_keys(
                    widths.iter().map(|(key, value)| {
                        (key.parse::<usize>().unwrap(), value.as_i64().unwrap())
                    }),
                );
            }
            if let Some(widths) = cfg["maximum"].as_array() {
                for pair in widths {
                    table.set_column_max_width(
                        pair[0].as_u64().unwrap() as usize,
                        pair[1].as_i64().unwrap(),
                    )?;
                }
            }
            if let Some(widths) = cfg["reset_widths"].as_array() {
                table.set_column_widths(widths.iter().map(|v| v.as_i64().unwrap()));
            }
            if let Some(format) = cfg["column_footer_format"].as_str() {
                table
                    .get_column_style_mut(42)
                    .set_footer_title_format(format);
            }
            if cfg.get("header").is_some() {
                table.set_header_title(cfg["header"].as_str());
            }
            if cfg.get("footer").is_some() {
                table.set_footer_title(cfg["footer"].as_str());
            }
            if let Some(format) = cfg["header_format"].as_str() {
                table.get_style_mut().set_header_title_format(format);
            }
            if let Some(format) = cfg["footer_format"].as_str() {
                table.get_style_mut().set_footer_title_format(format);
            }
            table.render()?;
            Ok(0)
        },
    )?;
    Artisan::command_with(
        "tables:hook {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            output.set_formatter(std::sync::Arc::new(TrackingFormatter {
                base: PlainFormatter(openrust::console::OutputFormatter::new(false)),
                calls: calls.clone(),
                answer: case["answer"].as_str().unwrap().to_owned(),
            }));
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output()).unwrap();
            table
                .set_headers(["A"])
                .set_rows([[case["text"].as_str().unwrap()]]);
            table.set_column_max_width(0, 2)?;
            table.render()?;
            assert_eq!(serde_json::json!(*calls.lock().unwrap()), case["calls"]);
            Ok(0)
        },
    )?;

    Ok(())
}
#[tokio::test]
async fn title_widths_and_wrapping_match_fixed_source_via_public_table_methods() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let mut failures = Vec::new();
    App::set_facade_application(Some(&app), async {
        for (index, case) in cases().into_iter().enumerate() {
            let mut output = Output::default();
            let flag = if case["decorated"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            let result = Artisan::call(
                "tables:layout",
                &[case.to_string().into(), flag.into()],
                &mut output,
            )
            .await;
            if case["error"] == true {
                let error = result.as_ref().expect_err("固定源失败路径");
                assert!(
                    matches!(
                        std::error::Error::source(error)
                            .and_then(|source| source.downcast_ref::<ConsoleTableError>()),
                        Some(
                            ConsoleTableError::InvalidColumnWidth(_)
                                | ConsoleTableError::UnsupportedWrappingFormatter
                        )
                    ),
                    "{error:?}"
                );
            }
            if result.is_err() != case["error"].as_bool().unwrap()
                || output.stdout != case["output"].as_str().unwrap()
            {
                failures.push(format!(
                    "case {index}: {case}\nactual:{:?},error:{result:?}",
                    output.stdout
                ));
            }
        }
    })
    .await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
#[ignore = "由实际Artisan子进程驱动"]
fn layout_cli_fixture() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::path::PathBuf::from(
        std::env::var_os("TABLE_LAYOUT_ROOT").unwrap(),
    ))
    .with_routing(Routing::default().commands(register))
    .create()
    .unwrap();
    rt.block_on(app.boot()).unwrap();
    let case: Value = serde_json::from_str(&std::env::var("TABLE_LAYOUT_CASE").unwrap()).unwrap();
    let flag = if case["decorated"] == true {
        "--ansi"
    } else {
        "--no-ansi"
    };
    let code = rt
        .block_on(app.handle_command(&[
            "tables:layout".into(),
            case.to_string().into(),
            flag.into(),
        ]))
        .unwrap();
    std::process::exit(code);
}
#[test]
fn actual_artisan_renders_titles_wrapped_cells_and_partial_failure_output() {
    let root = tempfile::tempdir().unwrap();
    let rows = cases();
    let selected = [
        rows.iter()
            .find(|r| {
                r["style"] == "default"
                    && r["decorated"] == false
                    && r["config"]["header"] == "Report"
            })
            .unwrap(),
        rows.iter()
            .find(|r| {
                r["style"] == "box"
                    && r["decorated"] == true
                    && r["config"]["maximum"] == serde_json::json!([[0, 3]])
            })
            .unwrap(),
        rows.iter()
            .find(|r| r["style"] == "markdown" && r["decorated"] == false && r["error"] == true)
            .unwrap(),
    ];
    for case in selected {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "layout_cli_fixture", "--ignored", "--nocapture"])
            .env("TABLE_LAYOUT_ROOT", root.path())
            .env("TABLE_LAYOUT_CASE", case.to_string())
            .output()
            .unwrap();
        let failure = case["error"] == true;
        assert_eq!(
            output.status.code(),
            Some(i32::from(failure)),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .strip_prefix("\nrunning 1 test\n")
                .unwrap(),
            case["output"].as_str().unwrap(),
            "{case}"
        );
        if failure {
            assert!(String::from_utf8_lossy(&output.stderr).contains("表格有效列宽"));
        } else {
            assert!(output.stderr.is_empty());
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for path in ["/table-layout", "/table-layout/error"] {
        let failure = path.ends_with("error");
        openrust::support::facades::Route::get(path, move || async move {
            let case = cases()
                .into_iter()
                .find(|case| {
                    case["style"] == "default"
                        && case["decorated"] == false
                        && if failure {
                            case["error"] == true
                        } else {
                            case["config"]["maximum"] == serde_json::json!([[0, 3]])
                        }
                })
                .unwrap();
            let mut output = Output::default();
            Artisan::call(
                "tables:layout",
                &[case.to_string().into(), "--no-ansi".into()],
                &mut output,
            )
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                serde_json::json!({"table":output.stdout}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_width_error_then_valid_layout_preserves_request_isolation() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register).web(routes))
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
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
        .unwrap();
    });
    for (path, status) in [
        ("/table-layout", 200),
        ("/table-layout/error", 500),
        ("/table-layout", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            let expected = cases()
                .into_iter()
                .find(|case| {
                    case["style"] == "default"
                        && case["decorated"] == false
                        && case["config"]["maximum"] == serde_json::json!([[0, 3]])
                })
                .unwrap();
            assert_eq!(
                response.json::<Value>().await.unwrap()["table"],
                expected["output"]
            );
        }
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn custom_wrapping_interface_dispatch_and_no_newline_result_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/table-wrappable-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases {
            let mut output = Output::default();
            assert_eq!(
                Artisan::call(
                    "tables:hook",
                    &[case.to_string().into(), "--no-ansi".into()],
                    &mut output
                )
                .await
                .unwrap(),
                0
            );
            assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
        }
    })
    .await;
}
