use openrust::{
    console::{ConsoleError, Input, Output, Table, TableRow, TableSeparator, TableStyle},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/table-separators-oracle.json"
    ))
    .unwrap()
}
fn row(value: &Value) -> TableRow {
    if value == "@sep" {
        TableSeparator::new().into()
    } else {
        value.as_array().unwrap().clone().into()
    }
}
fn rows(value: &Value) -> Vec<TableRow> {
    value.as_array().unwrap().iter().map(row).collect()
}
fn header_rows(value: &Value) -> Vec<Vec<Value>> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_array().unwrap().clone())
        .collect()
}
fn headers(table: &mut Table<'_>, value: &Value) {
    if value
        .as_array()
        .unwrap()
        .first()
        .is_some_and(Value::is_array)
    {
        table.set_headers_rows(header_rows(value));
    } else {
        table.set_headers(value.as_array().unwrap().clone());
    }
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:separators {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output()).unwrap();
            headers(&mut table, &case["headers"]);
            table
                .set_rows(rows(&case["rows"]))
                .set_style(TableStyle::from_name(case["style"].as_str().unwrap())?);
            match case["mode"].as_str().unwrap() {
                "horizontal" => {
                    table.set_horizontal(true);
                }
                "vertical" => {
                    table.set_vertical(true);
                }
                _ => {}
            }
            table
                .set_header_title(case["header"].as_str())
                .set_footer_title(case["footer"].as_str());
            for op in case["ops"].as_array().unwrap() {
                match op[0].as_str().unwrap() {
                    "addRow" => {
                        table.add_row(row(&op[1]))?;
                    }
                    "addRows" => {
                        table.add_rows(rows(&op[1]))?;
                    }
                    "setRows" => {
                        table.set_rows(rows(&op[1]));
                    }
                    "setRow" => {
                        table.set_row(op[1].as_i64().unwrap(), op[2].as_array().unwrap().clone());
                    }
                    _ => panic!("未知分隔行操作"),
                }
            }
            table.render()?;
            Ok(0)
        },
    )?;
    Ok(())
}

#[tokio::test]
async fn explicit_row_separators_all_styles_directions_keys_and_resets_match_source() {
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
                "tables:separators",
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
fn example(mode: &str) -> Value {
    cases()
        .into_iter()
        .find(|c| {
            c["style"] == "box-double"
                && c["decorated"] == false
                && c["mode"] == mode
                && c["headers"] == serde_json::json!(["A", "B"])
                && c["rows"] == serde_json::json!([["one", "two"], "@sep", ["three"]])
                && c.get("header").is_none()
        })
        .unwrap()
}
#[test]
#[ignore = "由真实Artisan父消费者驱动"]
fn separators_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let case = example(&std::env::var("TABLE_SEPARATORS_MODE").unwrap());
    std::process::exit(
        runtime
            .block_on(app.handle_command(&[
                "tables:separators".into(),
                case.to_string().into(),
                "--no-ansi".into(),
            ]))
            .unwrap(),
    );
}
#[test]
fn real_artisan_separators_and_convenience_methods_preserve_the_same_row_order() {
    for mode in ["default", "horizontal", "vertical"] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "separators_cli_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("TABLE_SEPARATORS_MODE", mode)
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
            example(mode)["output"].as_str().unwrap()
        );
    }
    let input = openrust::console::CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    input
        .io(&mut output)
        .table_named(
            ["A", "B"],
            [
                TableRow::from(["one", "two"]),
                TableSeparator::new().into(),
                TableRow::from(["three"]),
            ],
            "box-double",
        )
        .unwrap();
    assert_eq!(
        output.stdout,
        example("default")["output"].as_str().unwrap()
    );
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for mode in ["default", "horizontal", "vertical"] {
        openrust::support::facades::Route::get(
            &format!("/separator/{mode}"),
            move || async move {
                let case = example(mode);
                let mut output = Output::default();
                Artisan::call(
                    "tables:separators",
                    &[case.to_string().into(), "--no-ansi".into()],
                    &mut output,
                )
                .await
                .map_err(|error| openrust::http::HttpError::Internal {
                    source: Box::new(error),
                })?;
                openrust::http::Response::json(
                    http::StatusCode::OK,
                    serde_json::json!({"table":output.stdout}),
                )
            },
        )?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_all_orientations_use_public_separator_input() {
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
    for mode in ["default", "horizontal", "vertical", "default"] {
        let response = reqwest::get(format!("http://{address}/separator/{mode}"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<Value>().await.unwrap()["table"],
            example(mode)["output"]
        );
    }
    server.abort();
    let _ = server.await;
}
