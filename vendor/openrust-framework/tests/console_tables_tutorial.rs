use openrust::console::{CommandDefinition, Output, Table, TablePadding, TableStyle};
use openrust::{
    console::{ConsoleError, Input},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:probe {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let headers = case["headers"].as_array().unwrap().clone();
            let rows: Vec<Vec<Value>> = case["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row.as_array().unwrap().clone())
                .collect();
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output()).unwrap();
            table
                .set_headers(headers)
                .set_rows(rows)
                .set_style(style(case["style"].as_str().unwrap(), &case["config"])?);
            if let Some(columns) = case["columns"].as_object() {
                for (column, config) in columns {
                    table.set_column_style(column.parse().unwrap(), style("default", config)?);
                }
            }
            if let Some(columns) = case["columns"].as_array() {
                for (column, config) in columns.iter().enumerate() {
                    table.set_column_style(column, style("default", config)?);
                }
            }
            table.render()?;
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn table_presets_match_fixed_source() {
    let root = std::env::temp_dir().join(format!("tables-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut app = Application::configure(root.clone())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/table-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases.as_array().unwrap() {
            let mut output = Output::default();
            let flag = if case["decorated"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            assert_eq!(
                Artisan::call(
                    "tables:probe",
                    &[case.to_string().into(), flag.into()],
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
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn table_builder_reuses_output_and_validates_style() {
    let input = CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut io = input.io(&mut output);
    let mut table = Table::new(io.get_output()).unwrap();
    assert_eq!(table.get_style(), &TableStyle::default());
    table
        .set_headers(["Number"])
        .set_rows([[1]])
        .add_row([2])
        .unwrap()
        .render()
        .unwrap();
    assert!(TableStyle::from_name("missing").is_err());
    assert_eq!(
        output.stdout,
        "+--------+\n| Number |\n+--------+\n| 1      |\n| 2      |\n+--------+\n"
    );
}

fn style(name: &str, config: &Value) -> Result<TableStyle, ConsoleError> {
    let mut style = TableStyle::from_name(name)?;
    if let Some(config) = config.as_object() {
        for (key, value) in config {
            match key.as_str() {
                "pad_type" => {
                    style.set_pad_type(match value.as_i64().unwrap() {
                        0 => TablePadding::Left,
                        2 => TablePadding::Both,
                        _ => TablePadding::Right,
                    });
                }
                "padding_char" => {
                    style.set_padding_char(value.as_str().unwrap())?;
                }
                "cell_header_format" => {
                    style.set_cell_header_format(value.as_str().unwrap());
                }
                "cell_row_format" => {
                    style.set_cell_row_format(value.as_str().unwrap());
                }
                "cell_row_content_format" => {
                    style.set_cell_row_content_format(value.as_str().unwrap());
                }
                "border_format" => {
                    style.set_border_format(value.as_str().unwrap());
                }
                "horizontal" => {
                    style
                        .set_horizontal_border_chars(value[0].as_str().unwrap(), value[1].as_str());
                }
                "vertical" => {
                    style.set_vertical_border_chars(value[0].as_str().unwrap(), value[1].as_str());
                }
                "cross" => {
                    style.set_default_crossing_char(value.as_str().unwrap());
                }
                "outside" => {
                    style.set_display_outside_border(value.as_bool().unwrap());
                }
                _ => panic!("未知样式测试字段"),
            }
        }
    }
    Ok(style)
}

#[test]
fn table_cells_and_style_accessors_preserve_native_contracts() {
    use openrust::console::TableCell;
    for (value, expected) in [
        (serde_json::json!(true), "1"),
        (serde_json::json!(false), ""),
        (Value::Null, ""),
        (serde_json::json!(1.0), "1"),
        (serde_json::json!("text"), "text"),
    ] {
        assert_eq!(TableCell::new(value).as_str(), expected);
    }
    struct Label;
    impl std::fmt::Display for Label {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("label")
        }
    }
    assert_eq!(TableCell::display(Label).as_str(), "label");
    assert_eq!(TableCell::new(f64::NAN).as_str(), "NAN");
    assert_eq!(TableCell::new(f64::INFINITY).as_str(), "INF");
    assert_eq!(TableCell::new(f32::NEG_INFINITY).as_str(), "-INF");
    let mut style = TableStyle::default();
    assert!(style.set_padding_char("").is_err());
    assert_eq!(style.get_padding_char(), " ");
    style
        .set_pad_type(TablePadding::Both)
        .set_display_outside_border(false)
        .set_header_title_format("[%s]")
        .set_footer_title_format("(%s)");
    assert_eq!(style.get_pad_type(), TablePadding::Both);
    assert!(!style.display_outside_border());
    assert_eq!(style.get_header_title_format(), "[%s]");
    assert_eq!(style.get_footer_title_format(), "(%s)");
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    for (path, style) in [("/tables/default", "default"), ("/tables/error", "missing")] {
        Route::get(path, move || async move {
            let case = serde_json::json!({"style":style,"headers":["Name","Email"],"rows":[["Ada","ada@example.test"]]});
            let mut output = Output::default();
            Artisan::call("tables:probe", &[case.to_string().into()], &mut output)
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
async fn real_http_tables_errors_and_recovery_share_the_command_pipeline() {
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
    for path in ["/tables/default", "/tables/error", "/tables/default"] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("application/json")
        );
        if path.ends_with("error") {
            assert_eq!(response.status(), 500);
        } else {
            assert_eq!(response.status(), 200);
            let body = response.json::<Value>().await.unwrap();
            assert!(body["table"].as_str().unwrap().contains("ada@example.test"));
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn independent_handle_command_exercises_tables_and_exit_status() {
    let root = tempfile::tempdir().unwrap();
    for style in ["default", "missing", "box"] {
        let case = serde_json::json!({"style":style,"headers":["Name"],"rows":[["Ada"]]});
        let arguments = serde_json::json!(["tables:probe", case.to_string(), "--no-ansi"]);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "table_cli_fixture", "--ignored", "--nocapture"])
            .env("TABLE_CLI_ROOT", root.path())
            .env("TABLE_CLI_ARGUMENTS", arguments.to_string())
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(i32::from(style == "missing")));
        if style == "missing" {
            assert!(String::from_utf8_lossy(&result.stderr).contains("missing"));
        } else {
            assert!(String::from_utf8_lossy(&result.stdout).contains("Ada"));
        }
    }
}
#[test]
#[ignore = "由真实Artisan子进程验收调用"]
fn table_cli_fixture() {
    let root = std::env::var("TABLE_CLI_ROOT").unwrap();
    let arguments: Vec<std::ffi::OsString> =
        serde_json::from_str::<Vec<String>>(&std::env::var("TABLE_CLI_ARGUMENTS").unwrap())
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
    let app = Application::configure(root.into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&arguments)).unwrap();
    std::process::exit(code);
}

#[test]
fn table_helper_columns_and_typed_errors_are_real_consumers() {
    use openrust::console::ConsoleTableError;
    use std::error::Error as _;
    let input = CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut column = TableStyle::default();
    column.set_pad_type(TablePadding::Left);
    input
        .io(&mut output)
        .table_with_columns(["N"], [[1], [123]], TableStyle::default(), [(0, column)])
        .unwrap();
    assert!(output.stdout.contains("|   1 |"));
    let error = TableStyle::from_name("missing").unwrap_err();
    assert!(matches!(
        error.source().unwrap().downcast_ref::<ConsoleTableError>(),
        Some(ConsoleTableError::UndefinedStyle(_))
    ));
    let mut style = TableStyle::default();
    style.set_cell_row_content_format("%2$s");
    let error = input
        .io(&mut Output::default())
        .table_with(["N"], [[1]], style)
        .unwrap_err();
    assert!(matches!(
        error.source().unwrap().downcast_ref::<ConsoleTableError>(),
        Some(ConsoleTableError::InvalidFormat(_))
    ));
    assert!(error.source().unwrap().source().is_some());
    let mut style = TableStyle::default();
    style.set_padding_char("中").unwrap();
    let error = input
        .io(&mut Output::default())
        .table_with(["N"], [[1], [22]], style)
        .unwrap_err();
    assert!(matches!(
        error.source().unwrap().downcast_ref::<ConsoleTableError>(),
        Some(ConsoleTableError::InvalidPaddingEncoding(_))
    ));
}

#[tokio::test]
async fn queried_model_projection_flows_into_the_real_table_helper() {
    use openrust::{
        config::Repository,
        database::{
            DatabaseManager,
            eloquent::{Model, ModelMetadata, ModelState},
        },
    };
    use std::{collections::BTreeMap, sync::Arc};
    struct Contact(ModelState);
    impl Model for Contact {
        fn metadata() -> &'static ModelMetadata {
            static METADATA: ModelMetadata = ModelMetadata::new("contacts");
            &METADATA
        }
        fn from_state(state: ModelState) -> Self {
            Self(state)
        }
        fn state(&self) -> &ModelState {
            &self.0
        }
        fn state_mut(&mut self) -> &mut ModelState {
            &mut self.0
        }
    }
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        serde_json::json!({"default":"tables","connections":{"tables":{"driver":"sqlite","database":":memory:"}}}),
    )]));
    let db = DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap();
    db.statement(
        "CREATE TABLE contacts (id INTEGER PRIMARY KEY, name TEXT, email TEXT)",
        &[],
    )
    .await
    .unwrap();
    db.insert("INSERT INTO contacts (name,email) VALUES ('Ada','ada@example.test'),('Li','li@example.test')",&[]).await.unwrap();
    let contacts = Contact::query_on(&db)
        .select(["name", "email"])
        .get()
        .await
        .unwrap();
    let rows: Vec<_> = contacts
        .iter()
        .map(|contact| {
            let row = contact.to_array().unwrap();
            [row["name"].clone(), row["email"].clone()]
        })
        .collect();
    let input = CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    input
        .io(&mut output)
        .table(["Name", "Email"], rows)
        .unwrap();
    assert_eq!(
        output.stdout,
        "+------+------------------+\n| Name | Email            |\n+------+------------------+\n| Ada  | ada@example.test |\n| Li   | li@example.test  |\n+------+------------------+\n"
    );
}
