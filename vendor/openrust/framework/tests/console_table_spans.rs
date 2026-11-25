use openrust::{
    console::{
        ConsoleError, Input, Output, Table, TableCell, TableCellOptions, TableCellStyle, TableRow,
        TableSeparator, TableStyle,
    },
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-spans-oracle.json"
    ))
    .unwrap()
}
fn options(value: &Value) -> Result<TableCellOptions, ConsoleError> {
    let style = value
        .get("style")
        .filter(|v| !v.is_null())
        .map(|v| TableCellStyle::new(v.as_object().unwrap().clone()))
        .transpose()?;
    Ok(TableCellOptions {
        colspan: value.get("colspan").cloned().unwrap_or_else(|| 1.into()),
        rowspan: value.get("rowspan").cloned().unwrap_or_else(|| 1.into()),
        style,
    })
}
fn cell(value: &Value) -> Result<TableCell, ConsoleError> {
    if value["separator"] == true {
        return Ok(TableSeparator::new()
            .with_options(options(&value["options"])?)
            .into());
    }
    if let Some(text) = value.get("cell") {
        return Ok(TableCell::new(text.clone()).with_options(options(&value["options"])?));
    }
    Ok(value.clone().into())
}
fn cells(value: &Value) -> Result<Vec<TableCell>, ConsoleError> {
    value.as_array().unwrap().iter().map(cell).collect()
}
fn row(value: &Value) -> Result<TableRow, ConsoleError> {
    if value == "@sep" {
        Ok(TableSeparator::new().into())
    } else {
        Ok(cells(value)?.into())
    }
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:spans {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output())?;
            if case["headers"]
                .as_array()
                .unwrap()
                .first()
                .is_some_and(Value::is_array)
            {
                table.set_headers_rows(
                    case["headers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(cells)
                        .collect::<Result<Vec<_>, _>>()?,
                );
            } else {
                table.set_headers(cells(&case["headers"])?);
            }
            table
                .set_rows(
                    case["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(row)
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .set_style(TableStyle::from_name(case["style"].as_str().unwrap())?);
            if let Some(keyed) = case["keyed"].as_array() {
                for pair in keyed {
                    let row = cells(&pair[1])?;
                    if let Some(key) = pair[0].as_str() {
                        table.set_row(key, row);
                    } else {
                        table.set_row(pair[0].as_i64().unwrap(), row);
                    }
                }
            }
            for pair in case["minimum"].as_array().unwrap() {
                table.set_column_width(
                    pair[0].as_u64().unwrap() as usize,
                    pair[1].as_i64().unwrap(),
                );
            }
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
async fn spans_headers_directions_wrapping_styles_and_keyed_rows_match_fixed_source() {
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
                "tables:spans",
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
fn public_cell_and_inherited_separator_getters_match_source_scalar_conversions() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-span-getters-oracle.json"
    ))
    .unwrap();
    let mut runtime = 0;
    let mut compile_rejected = 0;
    for case in cases {
        if case["constructed"] == false {
            compile_rejected += 1;
            continue;
        }
        let opts = options(&case["options"]).unwrap();
        let (value, colspan, rowspan, style) = if case["separator"] == true {
            let separator = TableSeparator::new().with_options(opts);
            let style = separator.get_style().cloned();
            let colspan = separator.get_colspan();
            let rowspan = separator.get_rowspan();
            let value = TableCell::from(separator).to_string();
            (value, colspan, rowspan, style)
        } else {
            let cell = TableCell::new(case["value"].clone()).with_options(opts);
            (
                cell.to_string(),
                cell.get_colspan(),
                cell.get_rowspan(),
                cell.get_style().cloned(),
            )
        };
        assert_eq!(value, case["value_output"].as_str().unwrap(), "{case}");
        assert_eq!(colspan, case["colspan"].as_i64().unwrap(), "{case}");
        assert_eq!(rowspan, case["rowspan"].as_i64().unwrap(), "{case}");
        assert_eq!(
            serde_json::json!(style.map(|s| s.get_options())),
            case["style_output"],
            "{case}"
        );
        runtime += 1;
    }
    assert_eq!((runtime, compile_rejected), (22, 4));
    // 两种非法选项形状分别由正式类型上的 compile_fail 消费者验证，不能当作运行时通过。
    let default = TableCell::default();
    assert_eq!(
        (
            default.as_str(),
            default.get_colspan(),
            default.get_rowspan()
        ),
        ("", 1, 1)
    );
    let styled = TableCell::new("value")
        .with_colspan("2")
        .with_rowspan(2.8)
        .with_style(TableCellStyle::default());
    assert_eq!((styled.get_colspan(), styled.get_rowspan()), (2, 2));
    let reset = TableCell::new(styled);
    assert_eq!((reset.get_colspan(), reset.get_rowspan()), (1, 1));
    assert!(reset.get_style().is_none());
}
fn document_case(decorated: bool) -> Value {
    cases()
        .into_iter()
        .find(|case| case["decorated"] == decorated && case["headers"][0][0]["cell"] == "Summary")
        .unwrap()
}
fn invalid_case() -> Value {
    cases()
        .into_iter()
        .find(|case| {
            case["headers"] == serde_json::json!(["A", "B"])
                && case["rows"][0][0]["cell"] == "x"
                && case["rows"][0][0]["options"] == serde_json::json!({"rowspan":1,"colspan":0})
        })
        .unwrap()
}
#[test]
#[ignore = "由真实Artisan父消费者驱动"]
fn spans_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::path::PathBuf::from(
        std::env::var_os("TABLE_SPANS_ROOT").unwrap(),
    ))
    .with_routing(Routing::default().commands(register))
    .create()
    .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let args = [
        "tables:spans".into(),
        std::env::var("TABLE_SPANS_CASE").unwrap().into(),
        "--no-ansi".into(),
    ];
    std::process::exit(runtime.block_on(app.handle_command(&args)).unwrap());
}
#[test]
fn real_artisan_spans_and_invalid_layout_exit_status_match_source() {
    for case in [document_case(false), invalid_case()] {
        let root = tempfile::tempdir().unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "spans_cli_fixture", "--ignored", "--nocapture"])
            .env("TABLE_SPANS_ROOT", root.path())
            .env("TABLE_SPANS_CASE", case.to_string())
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
            assert!(String::from_utf8_lossy(&child.stderr).contains("跨度"));
        } else {
            assert!(child.stderr.is_empty());
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/table-spans", false), ("/table-spans/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let case = if invalid {
                invalid_case()
            } else {
                document_case(false)
            };
            let mut output = Output::default();
            Artisan::call(
                "tables:spans",
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
async fn real_http_spanning_headers_body_and_invalid_layout_recover() {
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
        ("/table-spans", 200),
        ("/table-spans/error", 500),
        ("/table-spans", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
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
#[tokio::test]
async fn repeated_render_preserves_input_and_source_column_minimum_mutations() {
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-spans-state-oracle.json"
    ))
    .unwrap();
    for case in expected {
        let input = openrust::console::CommandDefinition::from_signature("table")
            .unwrap()
            .parse(&[])
            .unwrap();
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let mut io = input.io(&mut output);
        let mut table = Table::new(io.get_output()).unwrap();
        table
            .set_headers(["A", "B"])
            .set_rows([[TableCell::new("one two three four")
                .with_colspan(2)
                .with_style(
                    TableCellStyle::new([
                        ("fg", serde_json::json!("green")),
                        ("align", serde_json::json!("right")),
                    ])
                    .unwrap(),
                )]])
            .set_style(TableStyle::from_name("box-double").unwrap());
        table.set_column_max_width(0, 4).unwrap();
        for _ in 0..3 {
            table.render().unwrap();
        }
        table.set_rows([["x", "y"]]).render().unwrap();
        table.set_column_widths([]).render().unwrap();
        let text: String = case["trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(output.stdout, text);
    }
}
#[test]
fn extreme_spans_report_layout_errors_instead_of_panicking_or_wrapping_indices() {
    let input = openrust::console::CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    for (colspan, rowspan) in [(i64::MAX, 1), (1, i64::MAX)] {
        let mut output = Output::default();
        let mut io = input.io(&mut output);
        let mut table = Table::new(io.get_output()).unwrap();
        table.set_rows([[TableCell::new("value")
            .with_colspan(colspan)
            .with_rowspan(rowspan)]]);
        assert!(table.render().is_err());
        assert!(output.stdout.is_empty());
    }
}
