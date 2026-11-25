use openrust::{
    console::{
        ConsoleError, Input, Output, Table, TableCell, TableCellStyle, TablePadding,
        TableSeparator, TableStyle,
    },
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-cell-styles-oracle.json"
    ))
    .unwrap()
}
fn map(value: &Value) -> serde_json::Map<String, Value> {
    if value == &serde_json::json!([]) {
        serde_json::Map::new()
    } else {
        value.as_object().unwrap().clone()
    }
}
fn cell(value: &Value) -> Result<TableCell, ConsoleError> {
    if value == "@cell-sep" {
        return Ok(TableSeparator::new().into());
    }
    if let Some(text) = value.get("cell") {
        let mut cell = TableCell::new(text.clone());
        if let Some(options) = value.get("style") {
            cell = cell.with_style(TableCellStyle::new(map(options))?);
        }
        Ok(cell)
    } else {
        Ok(value.clone().into())
    }
}
fn row(value: &Value) -> Result<Vec<TableCell>, ConsoleError> {
    value.as_array().unwrap().iter().map(cell).collect()
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:cell-styles {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output())?;
            table
                .set_headers(row(&case["headers"])?)
                .set_rows(
                    case["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(row)
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .set_style(TableStyle::from_name(case["style"].as_str().unwrap())?);
            for pair in case["maximum"].as_array().unwrap() {
                table.set_column_max_width(
                    pair[0].as_u64().unwrap() as usize,
                    pair[1].as_i64().unwrap(),
                )?;
            }
            match case["mode"].as_str().unwrap() {
                "horizontal" => {
                    table.set_horizontal(true);
                }
                "vertical" => {
                    table.set_vertical(true);
                }
                _ => {}
            }
            table.render()?;
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn cell_styles_headers_unicode_tags_wrapping_and_inner_separators_match_source() {
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
                "tables:cell-styles",
                &[case.to_string().into(), flag.into()],
                &mut output,
            )
            .await;
            if result.is_err() != case["error"].as_bool().unwrap()
                || output.stdout != case["output"].as_str().unwrap()
            {
                failures.push(format!(
                    "{index}: {case}\nactual:{:?}\nresult:{result:?}",
                    output.stdout
                ));
            }
        }
    })
    .await;
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
}
#[test]
fn public_cell_style_getters_defaults_errors_and_owned_options_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-cell-style-getters-oracle.json"
    ))
    .unwrap();
    for case in cases {
        let result = TableCellStyle::new(map(&case["input"]));
        assert_eq!(
            result.is_ok(),
            case["constructed"].as_bool().unwrap(),
            "{case}"
        );
        if let Ok(style) = result {
            assert_eq!(
                serde_json::json!(style.get_options()),
                case["options"],
                "{case}"
            );
            let expected = if case["tags"].is_array() {
                serde_json::json!({})
            } else {
                case["tags"].clone()
            };
            assert_eq!(
                serde_json::json!(style.get_tag_options()),
                expected,
                "{case}"
            );
            let pad = style.get_pad_by_align();
            assert_eq!(pad.is_err(), case["pad_error"] == true, "{case}");
            if let Ok(pad) = pad {
                assert_eq!(
                    match pad {
                        TablePadding::Left => 0,
                        TablePadding::Right => 1,
                        TablePadding::Both => 2,
                    },
                    case["pad"].as_i64().unwrap(),
                    "{case}"
                );
            }
            let format = style.get_cell_format();
            assert_eq!(format.is_err(), case["format_error"] == true, "{case}");
            if let Ok(format) = format {
                assert_eq!(serde_json::json!(format), case["format"], "{case}");
            }
            let mut detached = style.get_options();
            detached.insert("fg".into(), Value::String("mutated".into()));
            assert_eq!(serde_json::json!(style.get_options()), case["options"]);
        }
    }
    assert_eq!(TableCellStyle::DEFAULT_ALIGN, "left");
    let style = TableCellStyle::default();
    assert_eq!(style.get_pad_by_align().unwrap(), TablePadding::Right);
    let cell = TableCell::new("styled").with_style(style.clone());
    assert_eq!(cell.as_str(), "styled");
    assert_eq!(cell.get_style(), Some(&style));
    assert!(TableCell::new("plain").get_style().is_none());
}
struct RowMutationFormatter {
    base: openrust::console::OutputFormatter,
    style: TableStyle,

    fired: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Clone for RowMutationFormatter {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            style: TableStyle::from(&self.style),

            fired: self.fired.clone(),
        }
    }
}
impl openrust::console::OutputFormatterInterface for RowMutationFormatter {
    fn set_decorated(&self, decorated: bool) {
        self.base.set_decorated(decorated);
    }
    fn is_decorated(&self) -> bool {
        self.base.is_decorated()
    }
    fn set_style(&self, name: &str, style: openrust::console::FormatterStyle) {
        self.base.set_style(name, style);
    }
    fn has_style(&self, name: &str) -> bool {
        self.base.has_style(name)
    }
    fn get_style(
        &self,
        name: &str,
    ) -> Result<openrust::console::FormatterStyle, openrust::console::ConsoleFormatError> {
        self.base.get_style(name)
    }
    fn format(
        &self,
        message: Option<&str>,
    ) -> Result<Option<String>, openrust::console::ConsoleFormatError> {
        use std::sync::atomic::Ordering;
        if message == Some("one") && self.fired.fetch_add(1, Ordering::SeqCst) == 2 {
            TableStyle::from(&self.style).set_cell_row_format("[%s]");
        }
        Ok(Some(self.base.format_optional(message)?))
    }
}

#[test]
fn formatter_callback_row_format_snapshot_matches_real_source() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-cell-row-format-oracle.json"
    ))
    .unwrap();
    let style = TableStyle::default();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut output = Output::default();
    output.set_formatter(std::sync::Arc::new(RowMutationFormatter {
        base: openrust::console::OutputFormatter::new(false),
        style: TableStyle::from(&style),
        fired: seen.clone(),
    }));
    let input = openrust::console::CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut io = input.io(&mut output);
    Table::new(io.get_output())
        .unwrap()
        .set_headers(["A", "B"])
        .set_rows([["one", "two"], ["third", "last"]])
        .set_style(&style)
        .render()
        .unwrap();
    assert_eq!(output.stdout, expected["output"].as_str().unwrap());
    assert_eq!(
        seen.load(std::sync::atomic::Ordering::SeqCst),
        expected["seen"].as_u64().unwrap() as usize
    );
}
fn document_case(decorated: bool) -> Value {
    cases()
        .into_iter()
        .find(|c| {
            c["decorated"] == decorated && c["headers"] == serde_json::json!(["Name", "Score"])
        })
        .unwrap()
}
fn error_case() -> Value {
    cases()
        .into_iter()
        .find(|c| {
            c["decorated"] == false
                && c["style"] == "default"
                && c["mode"] == "default"
                && c["rows"][0][0]["cell"] == "long"
                && c["rows"][0][0]["style"] == serde_json::json!({"align":null})
        })
        .unwrap()
}
#[test]
#[ignore = "由真实Artisan父消费者驱动"]
fn cell_style_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::path::PathBuf::from(
        std::env::var_os("CELL_STYLE_ROOT").unwrap(),
    ))
    .with_routing(Routing::default().commands(register))
    .create()
    .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let args = [
        "tables:cell-styles".into(),
        std::env::var("CELL_STYLE_CASE").unwrap().into(),
        "--no-ansi".into(),
    ];
    std::process::exit(runtime.block_on(app.handle_command(&args)).unwrap());
}
#[test]
fn real_artisan_cell_styles_errors_exit_codes_and_partial_output_match_source() {
    for case in [document_case(false), error_case()] {
        let root = tempfile::tempdir().unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cell_style_cli_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("CELL_STYLE_ROOT", root.path())
            .env("CELL_STYLE_CASE", case.to_string())
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(if case["error"] == true { 1 } else { 0 }),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(
            String::from_utf8(child.stdout)
                .unwrap()
                .strip_prefix("\nrunning 1 test\n")
                .unwrap(),
            case["output"].as_str().unwrap()
        );
        if case["error"] == true {
            assert!(String::from_utf8_lossy(&child.stderr).contains("单元格对齐值无效"));
        } else {
            assert!(child.stderr.is_empty());
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, error) in [("/cell-styles", false), ("/cell-styles/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let case = if error {
                error_case()
            } else {
                document_case(false)
            };
            let mut output = Output::default();
            Artisan::call(
                "tables:cell-styles",
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
async fn real_http_style_configuration_failure_falls_back_without_poisoning_later_commands() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register).web(routes))
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
        .unwrap();
    });
    for (path, status) in [
        ("/cell-styles", 200),
        ("/cell-styles/error", 500),
        ("/cell-styles", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("application/json")
        );
        if status == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["table"],
                document_case(false)["output"]
            );
        }
    }
    server.abort();
    let _ = server.await;
}
struct LiveRowFormatter {
    base: openrust::console::OutputFormatter,
    style: TableStyle,
    mode: String,
    seen: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    fired: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Clone for LiveRowFormatter {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            style: TableStyle::from(&self.style),
            mode: self.mode.clone(),
            seen: self.seen.clone(),
            fired: self.fired.clone(),
        }
    }
}
impl openrust::console::OutputFormatterInterface for LiveRowFormatter {
    fn set_decorated(&self, value: bool) {
        self.base.set_decorated(value);
    }
    fn is_decorated(&self) -> bool {
        self.base.is_decorated()
    }
    fn set_style(&self, name: &str, style: openrust::console::FormatterStyle) {
        self.base.set_style(name, style);
    }
    fn has_style(&self, name: &str) -> bool {
        self.base.has_style(name)
    }
    fn get_style(
        &self,
        name: &str,
    ) -> Result<openrust::console::FormatterStyle, openrust::console::ConsoleFormatError> {
        self.base.get_style(name)
    }
    fn format(
        &self,
        message: Option<&str>,
    ) -> Result<Option<String>, openrust::console::ConsoleFormatError> {
        use std::sync::atomic::Ordering;
        let text = message.unwrap_or("");
        let trigger = if self.mode == "line" {
            text.starts_with('|') && text.contains("one")
        } else {
            text == "one" && self.seen.fetch_add(1, Ordering::SeqCst) == 2
        };
        if trigger && !self.fired.swap(true, Ordering::SeqCst) {
            TableStyle::from(&self.style)
                .set_cell_row_format("[%s]")
                .set_vertical_border_chars("!", None);
        }
        Ok(Some(self.base.format_optional(message)?))
    }
}
#[test]
fn live_border_changes_and_physical_line_format_snapshots_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-cell-live-format-oracle.json"
    ))
    .unwrap();
    for case in cases {
        let style = TableStyle::default();
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut output = Output::default();
        output.set_formatter(std::sync::Arc::new(LiveRowFormatter {
            base: openrust::console::OutputFormatter::new(false),
            style: TableStyle::from(&style),
            mode: case["mode"].as_str().unwrap().to_owned(),
            seen: seen.clone(),
            fired: fired.clone(),
        }));
        let input = openrust::console::CommandDefinition::from_signature("table")
            .unwrap()
            .parse(&[])
            .unwrap();
        let mut io = input.io(&mut output);
        let mut table = Table::new(io.get_output()).unwrap();
        table.set_style(&style).set_headers(["A", "B"]);
        if case["mode"] == "line" {
            table.set_rows([["one\nnext", "two\nlast"]]);
        } else {
            table.set_rows([["one", "two"], ["third", "last"]]);
        }
        table.render().unwrap();
        assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
        assert_eq!(
            seen.load(std::sync::atomic::Ordering::SeqCst),
            case["seen"].as_u64().unwrap() as usize
        );
        assert_eq!(
            fired.load(std::sync::atomic::Ordering::SeqCst),
            case["fired"].as_bool().unwrap()
        );
    }
}
