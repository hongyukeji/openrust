use openrust::{
    console::{ConsoleError, ConsoleTableError, Input, Output, Table, TableStyle},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-rows-oracle.json"
    ))
    .unwrap()
}
fn rows(value: &Value) -> Vec<Vec<Value>> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_array().unwrap().clone())
        .collect()
}
fn headers(table: &mut Table<'_, '_>, value: &Value) {
    if value
        .as_array()
        .unwrap()
        .first()
        .is_some_and(Value::is_array)
    {
        table.set_headers_rows(rows(value));
    } else {
        table.set_headers(value.as_array().unwrap().clone());
    }
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:rows {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output());
            headers(&mut table, &case["headers"]);
            table
                .set_rows(rows(&case["rows"]))
                .set_style(TableStyle::from_name(case["style"].as_str().unwrap())?);
            if let Some(options) = case.get("options") {
                for pair in options["maximum"].as_array().unwrap() {
                    table.set_column_max_width(
                        pair[0].as_u64().unwrap() as usize,
                        pair[1].as_i64().unwrap(),
                    )?;
                }
                table
                    .set_header_title(options["header"].as_str())
                    .set_footer_title(options["footer"].as_str());
                if options["column_style"] == true {
                    let mut style = TableStyle::default();
                    style
                        .set_padding_char(".")?
                        .set_pad_type(openrust::console::TablePadding::Left)
                        .set_cell_row_content_format("[%s]")
                        .set_horizontal_border_chars("~", None);
                    table.set_column_style(0, style);
                }
            }
            let mut trace = Vec::new();
            for op in case["ops"].as_array().unwrap() {
                let result = match op[0].as_str().unwrap() {
                    "addRows" => table.add_rows(rows(&op[1])).map(|_| ()),
                    "addRow" => table.add_row(op[1].as_array().unwrap().clone()).map(|_| ()),
                    "setRow" => {
                        let row = op[2].as_array().unwrap().clone();
                        if let Some(key) = op[1].as_str() {
                            table.set_row(key, row);
                        } else {
                            table.set_row(op[1].as_i64().unwrap(), row);
                        }
                        Ok(())
                    }
                    "setRows" => {
                        table.set_rows(rows(&op[1]));
                        Ok(())
                    }
                    "headers" => {
                        headers(&mut table, &op[1]);
                        Ok(())
                    }
                    "horizontal" => {
                        table.set_horizontal(op[1].as_bool().unwrap());
                        Ok(())
                    }
                    "vertical" => {
                        table.set_vertical(op[1].as_bool().unwrap());
                        Ok(())
                    }
                    _ => panic!("未知行操作"),
                };
                if let Err(error) = &result {
                    assert!(matches!(
                        std::error::Error::source(error)
                            .and_then(|error| error.downcast_ref::<ConsoleTableError>()),
                        Some(ConsoleTableError::RowIndexOverflow)
                    ));
                }
                trace.push(result.is_err());
            }
            assert_eq!(serde_json::json!(trace), case["trace"], "{case}");
            table.render()?;
            Ok(0)
        },
    )?;
    Artisan::command_with("tables:overflow", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output());
        table.set_row(i64::MAX, ["last"]);
        table.add_row(["overflow"])?;
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
#[tokio::test]
async fn row_keys_overflow_resets_headers_and_orientations_match_real_source() {
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
                "tables:rows",
                &[case.to_string().into(), flag.into()],
                &mut output,
            )
            .await;
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
#[ignore = "由实际Artisan父消费者驱动"]
fn rows_cli_fixture() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::path::PathBuf::from(
        std::env::var_os("TABLE_ROWS_ROOT").unwrap(),
    ))
    .with_routing(Routing::default().commands(register))
    .create()
    .unwrap();
    rt.block_on(app.boot()).unwrap();
    let args = if std::env::var_os("TABLE_ROWS_OVERFLOW").is_some() {
        vec!["tables:overflow".into(), "--no-ansi".into()]
    } else {
        let case: Value = serde_json::from_str(&std::env::var("TABLE_ROWS_CASE").unwrap()).unwrap();
        let flag = if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        };
        vec!["tables:rows".into(), case.to_string().into(), flag.into()]
    };
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    std::process::exit(code);
}
#[test]
fn real_artisan_orientations_multi_headers_and_overflow_exit_status() {
    let root = tempfile::tempdir().unwrap();
    let all = cases();
    for case in [
        all.iter()
            .find(|c| {
                c["style"] == "box"
                    && c["decorated"] == true
                    && c["ops"] == serde_json::json!([["horizontal", true]])
            })
            .unwrap(),
        all.iter()
            .find(|c| {
                c["style"] == "default"
                    && c["decorated"] == false
                    && c["ops"] == serde_json::json!([["vertical", true]])
            })
            .unwrap(),
        all.iter()
            .find(|c| {
                c["style"] == "box"
                    && c["decorated"] == false
                    && c["ops"] == serde_json::json!([["headers", [["H1", "H2"], ["H3"]]]])
            })
            .unwrap(),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "rows_cli_fixture", "--ignored", "--nocapture"])
            .env("TABLE_ROWS_ROOT", root.path())
            .env_remove("TABLE_ROWS_OVERFLOW")
            .env("TABLE_ROWS_CASE", case.to_string())
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(child.stderr.is_empty());
        assert_eq!(
            String::from_utf8(child.stdout)
                .unwrap()
                .strip_prefix("\nrunning 1 test\n")
                .unwrap(),
            case["output"].as_str().unwrap()
        );
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "rows_cli_fixture", "--ignored", "--nocapture"])
        .env("TABLE_ROWS_ROOT", root.path())
        .env("TABLE_ROWS_OVERFLOW", "1")
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&child.stderr).contains("数字索引已达到上限"));
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for path in ["/table-rows", "/table-rows/error"] {
        let error = path.ends_with("error");
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let (name, args) = if error {
                ("tables:overflow", Vec::new())
            } else {
                let case = cases()
                    .into_iter()
                    .find(|c| {
                        c["style"] == "default"
                            && c["decorated"] == false
                            && c["ops"] == serde_json::json!([["vertical", true]])
                    })
                    .unwrap();
                (
                    "tables:rows",
                    vec![case.to_string().into(), "--no-ansi".into()],
                )
            };
            Artisan::call(name, &args, &mut output)
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
async fn real_http_overflow_does_not_poison_later_row_rendering() {
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
        ("/table-rows", 200),
        ("/table-rows/error", 500),
        ("/table-rows", 200),
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
                .find(|c| {
                    c["style"] == "default"
                        && c["decorated"] == false
                        && c["ops"] == serde_json::json!([["vertical", true]])
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
