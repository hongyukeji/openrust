use openrust::console::{
    AsOutput, CommandDefinition, ConsoleError, DefinitionListItem, Output, OutputReference,
    OutputStyle, OutputVerbosity, TableCell, TableHeaders, TableRow, TableSeparator,
    contracts::NewLineAware,
};
use serde_json::{Value, json};
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/style-table-oracle.json"
    ))
    .unwrap()
}
fn inputs(v: u64) -> (TableHeaders, Vec<TableRow>, Vec<DefinitionListItem>) {
    let headers: TableHeaders = match v {
        0 => ["Name", "Value"].into(),
        1 => Vec::<TableCell>::new().into(),
        2 => ["<info>名</info>", "Count"].into(),
        3 => ["One", "Two"].into(),
        4 => ["Only"].into(),
        5 => ["A", "B"].into(),
        6 => ["H", "V"].into(),
        7 => TableHeaders::rows([["A", "B"], ["C", "D"]]),
        8 => [
            TableCell::from("Title").with_colspan(2),
            TableCell::from("Name"),
        ]
        .into(),
        _ => unreachable!(),
    };
    let rows: Vec<TableRow> = match v {
        0 => vec![
            vec![TableCell::from("Ada"), 42.into()].into(),
            vec![TableCell::from("Lin"), Value::Null.into()].into(),
        ],
        1 => vec![],
        2 => vec![vec![TableCell::from("a\nb"), true.into()].into()],
        3 => vec![
            ["x", "y"].into(),
            TableSeparator::new().into(),
            ["z", "q"].into(),
        ],
        4 => vec![["x", "y", "z"].into()],
        5 => vec![[TableCell::from("wide").with_colspan(2)].into()],
        6 => vec![["<fg=bogus>x</>", "y"].into()],
        7 => vec![["x", "y"].into()],
        8 => vec![
            vec![TableCell::from(Value::Null), TableCell::from("x")].into(),
            vec![TableCell::from(""), TableCell::from("y")].into(),
        ],
        _ => unreachable!(),
    };
    let list = match v {
        0 => vec![
            "Profile".into(),
            DefinitionListItem::entry("Name", "Ada"),
            DefinitionListItem::entry("Count", 42),
        ],
        1 => vec![],
        2 => vec![
            DefinitionListItem::entry("名", "a\nb"),
            DefinitionListItem::entry("Enabled", true),
        ],
        3 => vec![
            DefinitionListItem::entry("A", "x"),
            TableSeparator::new().into(),
            DefinitionListItem::entry("B", "y"),
        ],
        4 => vec![
            DefinitionListItem::Entries(vec![]),
            DefinitionListItem::entry("second", "value"),
        ],
        5 => vec![
            DefinitionListItem::entries([("first", "used"), ("second", "ignored")]),
            "Footer".into(),
        ],
        6 => vec![DefinitionListItem::entry("<fg=bogus>x</>", "y")],
        7 => vec![
            "Title".into(),
            TableSeparator::new().into(),
            DefinitionListItem::entry("Key", Value::Null),
        ],
        8 => vec![
            "Title".into(),
            DefinitionListItem::entry("Empty", ""),
            DefinitionListItem::entry("Null", Value::Null),
            DefinitionListItem::entry("False", false),
        ],
        _ => unreachable!(),
    };
    (headers, rows, list)
}
fn execute(style: &mut OutputStyle<'_>, c: &Value) -> Result<(), ConsoleError> {
    let (h, r, l) = inputs(c["variant"].as_u64().unwrap());
    match c["mode"].as_str().unwrap() {
        "table" => style.table(h, r),
        "horizontal" => style.horizontal_table(h, r),
        "definition" => style.definition_list(l),
        _ => unreachable!(),
    }
}
fn parent_counts(output: &mut OutputReference<'_>) -> Vec<Value> {
    let mut result = Vec::new();
    if let Some(view) = output.as_new_line_aware() {
        result.push(json!([view.new_lines_written(), view.new_line_written()]));
        result.extend(parent_counts(&mut output.get_output().unwrap()));
    }
    result
}
fn run_case(style: &mut OutputStyle<'_>, c: &Value) -> Value {
    let result = execute(style, c);
    if let Err(error) = &result {
        let mut current: &(dyn std::error::Error + 'static) = error;
        let mut found = false;
        loop {
            if current
                .downcast_ref::<openrust::console::ConsoleFormatError>()
                .is_some()
            {
                found = true;
                break;
            }
            if let Some(next) = current.source() {
                current = next
            } else {
                break;
            }
        }
        assert!(found, "{error:?}");
    }
    let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
    counts.extend(parent_counts(style.get_output()));
    counts.reverse();
    json!({"output":style.get_output().stdout,"counts":counts,"failed":result.is_err()})
}
#[test]
fn fixed_source_style_tables_and_definition_lists() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "style_tables_driver", "--ignored", "--nocapture"])
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
fn style_tables_driver() {
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let input = CommandDefinition::from_signature("style-tables")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut failures = Vec::new();
    for (index, c) in cases().iter().enumerate() {
        let mut output = Output::default();
        output.set_verbosity(if c["level"] == 16 {
            OutputVerbosity::Quiet
        } else {
            OutputVerbosity::Normal
        });
        output.set_decorated(c["decorated"].as_bool().unwrap());
        let mut section;
        let target: &mut dyn AsOutput = if c["section"] == true {
            section = output.section();
            &mut section
        } else {
            &mut output
        };
        let mut first = OutputStyle::new(&input, target);
        let actual = if c["depth"] == 2 {
            let mut second = OutputStyle::new(&input, &mut first);
            run_case(&mut second, c)
        } else {
            run_case(&mut first, c)
        };
        let expected =
            json!({"output":c["output"],"counts":c["counts"],"failed":c["error"].is_string()});
        if actual != expected {
            failures.push(format!(
                "case{index}: actual{actual:?},expected{expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
fn register() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:tables {mode} {variant}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("mode")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap();
                let variant = input
                    .argument("variant")
                    .and_then(|value| value.as_str().and_then(|value| value.parse::<u64>().ok()))
                    .unwrap();
                let mut style = OutputStyle::new(input, output);
                let result = execute(&mut style, &json!({"mode":mode,"variant":variant}));
                if std::env::var_os("STYLE_TABLE_CLI").is_some() {
                    eprintln!(
                        "STYLE_TABLE_COUNTS:{}",
                        json!([style.new_lines_written(), style.new_line_written()])
                    );
                }
                result?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn source_case(mode: &str, variant: u64, decorated: bool, level: u64) -> Value {
    cases()
        .into_iter()
        .find(|c| {
            c["mode"] == mode
                && c["variant"] == variant
                && c["depth"] == 1
                && c["section"] == false
                && c["decorated"] == decorated
                && c["level"] == level
        })
        .unwrap()
}
#[test]
fn actual_artisan_table_helpers_use_real_process_status_and_source_bytes() {
    for mode in ["table", "horizontal", "definition"] {
        for variant in [0, 6] {
            for decorated in [false, true] {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "style_tables_cli_driver",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env(
                        "STYLE_TABLE_CLI",
                        json!({"mode":mode,"variant":variant,"decorated":decorated}).to_string(),
                    )
                    .env("COLUMNS", "80")
                    .output()
                    .unwrap();
                let expected = source_case(mode, variant, decorated, 32);
                assert_eq!(
                    child.status.code(),
                    Some(if variant == 6 { 1 } else { 0 }),
                    "{}",
                    String::from_utf8_lossy(&child.stderr)
                );
                let stdout = String::from_utf8(child.stdout).unwrap();
                let bytes = stdout.split_once("STYLE_TABLE_BEGIN").unwrap().1;
                assert_eq!(
                    bytes,
                    expected["output"].as_str().unwrap(),
                    "{mode}/{variant}/{decorated}"
                );
                let stderr = String::from_utf8(child.stderr).unwrap();
                let counts: Value = serde_json::from_str(
                    stderr
                        .lines()
                        .find_map(|l| l.strip_prefix("STYLE_TABLE_COUNTS:"))
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(counts, expected["counts"][0]);
            }
        }
    }
}
#[test]
#[ignore = "由父消费者真实启动独立Artisan"]
fn style_tables_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let c: Value = serde_json::from_str(&std::env::var("STYLE_TABLE_CLI").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(app.boot()).unwrap();
    print!("STYLE_TABLE_BEGIN");
    std::io::stdout().flush().unwrap();
    let status = rt
        .block_on(app.handle_command(&[
            "styles:tables".into(),
            c["mode"].as_str().unwrap().into(),
            c["variant"].to_string().into(),
            if c["decorated"] == true {
                "--ansi".into()
            } else {
                "--no-ansi".into()
            },
        ]))
        .unwrap();
    drop(rt);
    drop(root);
    std::process::exit(status);
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    for mode in ["table", "horizontal", "definition"] {
        for variant in [0, 6] {
            let path = format!("/style-tables/{mode}/{variant}");
            openrust::support::facades::Route::get(&path, move || async move {
                let mut output = Output::default();
                openrust::support::facades::Artisan::call(
                    "styles:tables",
                    &[mode.into(), variant.to_string().into(), "--no-ansi".into()],
                    &mut output,
                )
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
                openrust::http::Response::json(
                    http::StatusCode::OK,
                    json!({"output":output.stdout}),
                )
            })?;
        }
    }
    Ok(())
}
#[tokio::test]
async fn real_http_style_tables_and_definition_errors_recover_and_remain_isolated() {
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
        .unwrap()
    });
    for mode in ["table", "horizontal", "definition"] {
        for variant in [0, 6, 0] {
            let response = reqwest::Client::new()
                .get(format!("http://{address}/style-tables/{mode}/{variant}"))
                .header("Accept", "application/json")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), if variant == 6 { 500 } else { 200 });
            assert!(
                response.headers()[reqwest::header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with("application/json")
            );
            let body: Value = response.json().await.unwrap();
            if variant == 0 {
                assert_eq!(
                    body,
                    json!({"output":source_case(mode,0,false,32)["output"]})
                );
            } else {
                assert!(body.is_object());
            }
        }
    }
    let mut tasks = Vec::new();
    for n in 0..12 {
        tasks.push(tokio::spawn(async move {
            let mode = ["table", "horizontal", "definition"][n % 3];
            let body: Value = reqwest::get(format!("http://{address}/style-tables/{mode}/0"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(
                body,
                json!({"output":source_case(mode,0,false,32)["output"]})
            );
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
#[test]
fn typed_headers_and_definition_items_consume_input_once() {
    let input = CommandDefinition::from_signature("one-pass")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let calls = std::cell::Cell::new(0);
    let headers = TableHeaders::rows(
        [["A", "B"], ["C", "D"]]
            .into_iter()
            .inspect(|_| calls.set(calls.get() + 1)),
    );
    assert_eq!(calls.get(), 2);
    let mut style = OutputStyle::new(&input, &mut output);
    style.table(headers, [["x", "y"]]).unwrap();
    assert_eq!(calls.get(), 2);
    let items = [
        DefinitionListItem::entries([("first", "used"), ("second", "ignored")]),
        "Footer".into(),
    ];
    style
        .definition_list(items.into_iter().inspect(|_| calls.set(calls.get() + 1)))
        .unwrap();
    assert_eq!(calls.get(), 4);
    assert!(!style.get_output().stdout.contains("ignored"));
}
