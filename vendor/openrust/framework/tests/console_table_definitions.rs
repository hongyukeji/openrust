use openrust::{
    console::{ConsoleError, ConsoleTableError, Input, Output, Table, TablePadding, TableStyle},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-definitions-oracle.json"
    ))
    .unwrap()
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:definitions {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut io = input.io(output);
            let mut table = Table::new(io.get_output())?;
            table.set_headers(["A", "B"]).set_rows([["one", "two"]]);
            let mut external = TableStyle::default();
            let mut alias = TableStyle::default();
            let mut clone = TableStyle::default();
            let mut errors = Vec::new();
            for op in case["ops"].as_array().unwrap() {
                let result: Result<(), ConsoleError> = match op[0].as_str().unwrap() {
                    "render" => table.render(),
                    "define" | "replace" => Table::set_style_definition(
                        op[1].as_str().unwrap(),
                        TableStyle::from_name(op[2].as_str().unwrap())?,
                    ),
                    "named" => table.set_style_named(op[1].as_str().unwrap()).map(|_| ()),
                    "mutate" => {
                        Table::get_style_definition(op[1].as_str().unwrap()).map(|mut style| {
                            style.set_horizontal_border_chars(op[2].as_str().unwrap(), None);
                        })
                    }
                    "external" => {
                        external = TableStyle::from_name(op[1].as_str().unwrap())?;
                        table.set_style(&external);
                        Ok(())
                    }
                    "define_external" => {
                        Table::set_style_definition(op[1].as_str().unwrap(), &external)
                    }
                    "external_mutate" => {
                        external.set_horizontal_border_chars(op[1].as_str().unwrap(), None);
                        Ok(())
                    }
                    "clone" => {
                        table.set_style(external.clone());
                        Ok(())
                    }
                    "column" => table
                        .set_column_style_named(
                            op[1].as_u64().unwrap() as usize,
                            op[2].as_str().unwrap(),
                        )
                        .map(|_| ()),
                    "alias_global" => {
                        alias = TableStyle::from(table.get_style());
                        Ok(())
                    }
                    "alias_column" => {
                        alias = TableStyle::from(
                            table.get_column_style(op[1].as_u64().unwrap() as usize),
                        );
                        Ok(())
                    }
                    "alias_mutate" => {
                        alias
                            .set_padding_char(op[1].as_str().unwrap())
                            .map(|style| {
                                style.set_pad_type(TablePadding::Left);
                            })
                    }
                    "clone_definition" => {
                        clone = Table::get_style_definition(op[1].as_str().unwrap())?.clone();
                        Ok(())
                    }
                    "clone_mutate" => {
                        clone.set_horizontal_border_chars(op[1].as_str().unwrap(), None);
                        Ok(())
                    }
                    _ => panic!("未知样式操作"),
                };
                if let Err(error) = &result {
                    assert!(matches!(
                        std::error::Error::source(error)
                            .and_then(|e| e.downcast_ref::<ConsoleTableError>()),
                        Some(ConsoleTableError::UndefinedStyle(_))
                    ));
                }
                errors.push(result.is_err());
            }
            assert_eq!(serde_json::json!(errors), case["errors"]);
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn named_styles_aliasing_replacement_clone_and_columns_match_actual_source() {
    for case in cases() {
        let root = tempfile::tempdir().unwrap();
        let mut app = Application::configure(root.path().into())
            .with_routing(Routing::default().commands(register))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        App::set_facade_application(Some(&app), async {
            let mut output = Output::default();
            let flag = if case["decorated"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            Artisan::call(
                "tables:definitions",
                &[case.to_string().into(), flag.into()],
                &mut output,
            )
            .await
            .unwrap();
            let expected: String = case["trace"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            assert_eq!(output.stdout, expected, "{case}");
        })
        .await;
    }
}
#[tokio::test]
async fn application_registry_isolation_lifecycle_and_standalone_defaults() {
    assert!(Table::get_style_definition("default").is_err());
    assert!(Table::set_style_definition("outside", TableStyle::default()).is_err());
    let mut output = Output::default();
    let input = openrust::console::CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut io = input.io(&mut output);
    let mut standalone = Table::new(io.get_output()).unwrap();
    standalone.set_style_named("box").unwrap();
    assert!(standalone.set_style_named("outside").is_err());
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("a")).unwrap();
    std::fs::create_dir_all(root.path().join("b")).unwrap();
    let app_a = Application::configure(root.path().join("a"))
        .create()
        .unwrap();
    let app_b = Application::configure(root.path().join("b"))
        .create()
        .unwrap();
    let mut output = Output::default();
    let mut io = input.io(&mut output);
    let mut table = App::set_facade_application(Some(&app_a), async {
        Table::set_style_definition("a", TableStyle::from_name("box").unwrap()).unwrap();
        let mut table = Table::new(io.get_output()).unwrap();
        table.set_style_named("a").unwrap();
        table
    })
    .await;
    // 实例保留创建应用的定义，不随外层作用域切换。
    App::set_facade_application(Some(&app_b), async {
        assert!(Table::get_style_definition("a").is_err());
        Table::set_style_definition("a", TableStyle::from_name("compact").unwrap()).unwrap();
        table.set_style_named("a").unwrap();
        assert_eq!(table.get_style().get_border_chars()[0], "─");
    })
    .await;
    drop(app_a);
    table.set_style_named("a").unwrap();
    assert_eq!(table.get_style().get_border_chars()[0], "─");
    assert!(Table::get_style_definition("a").is_err());
}
#[test]
#[ignore = "由真实Artisan父消费者驱动"]
fn definitions_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let args = [
        "tables:definitions".into(),
        std::env::var("TABLE_DEFINITIONS_CASE").unwrap().into(),
        "--no-ansi".into(),
    ];
    std::process::exit(runtime.block_on(app.handle_command(&args)).unwrap());
}
#[test]
fn real_artisan_named_registry_consumer() {
    let case = cases()
        .into_iter()
        .find(|c| {
            c["decorated"] == false && c["ops"][0][0] == "define" && c["ops"][2][0] == "replace"
        })
        .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "definitions_cli_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("TABLE_DEFINITIONS_CASE", case.to_string())
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(child.stderr.is_empty());
    let expected: String = case["trace"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(
        String::from_utf8(child.stdout)
            .unwrap()
            .strip_prefix("\nrunning 1 test\n")
            .unwrap(),
        expected
    );
}
fn routes_a() -> Result<(), openrust::routing::RoutingError> {
    routes(true)
}
fn routes_b() -> Result<(), openrust::routing::RoutingError> {
    routes(false)
}
fn routes(mutate: bool) -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/styles", move || async move {
        if mutate {
            Table::get_style_definition("default")
                .map_err(internal)?
                .set_horizontal_border_chars("~", None);
        }
        let case = cases()
            .into_iter()
            .find(|c| c["decorated"] == false && c["ops"] == serde_json::json!([["render"]]))
            .unwrap();
        let mut output = Output::default();
        Artisan::call(
            "tables:definitions",
            &[case.to_string().into(), "--no-ansi".into()],
            &mut output,
        )
        .await
        .map_err(internal)?;
        openrust::http::Response::json(
            http::StatusCode::OK,
            serde_json::json!({"table": output.stdout}),
        )
    })?;
    Ok(())
}
fn internal(error: ConsoleError) -> openrust::http::HttpError {
    openrust::http::HttpError::Internal {
        source: Box::new(error),
    }
}
#[tokio::test]
async fn real_http_two_application_registries_do_not_leak() {
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
    let mut servers = Vec::new();
    let mut addresses = Vec::new();
    for (name, routes) in [
        (
            "a",
            routes_a as fn() -> Result<(), openrust::routing::RoutingError>,
        ),
        ("b", routes_b),
    ] {
        std::fs::create_dir_all(root.path().join(name)).unwrap();
        let app = Application::configure(root.path().join(name))
            .with_routing(Routing::default().commands(register).web(routes))
            .create()
            .unwrap();
        let mut kernel = openrust::foundation::http::Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        addresses.push(listener.local_addr().unwrap());
        servers.push(tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        }));
    }
    let all = cases();
    let plain = all
        .iter()
        .find(|c| c["decorated"] == false && c["ops"] == serde_json::json!([["render"]]))
        .unwrap()["trace"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let changed = all
        .iter()
        .find(|c| {
            c["decorated"] == false
                && c["ops"]
                    == serde_json::json!([
                        ["named", "default"],
                        ["mutate", "default", "~"],
                        ["render"]
                    ])
        })
        .unwrap()["trace"][2]
        .as_str()
        .unwrap()
        .to_owned();
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..24 {
        let app = index % 2;
        let url = format!("http://{}/styles", addresses[app]);
        let expected = if app == 0 {
            changed.clone()
        } else {
            plain.clone()
        };
        requests.spawn(async move {
            let response = reqwest::get(url).await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.json::<Value>().await.unwrap()["table"], expected);
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    for server in servers {
        server.abort();
        let _ = server.await;
    }
    assert!(Table::get_style_definition("default").is_err());
}
#[tokio::test]
async fn convenience_helpers_resolve_application_defaults_and_named_column_styles() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into()).create().unwrap();
    App::set_facade_application(Some(&app), async {
        let mut default = Table::get_style_definition("default").unwrap();
        default.set_horizontal_border_chars("~", None);
        Table::set_style_definition("custom", TableStyle::from_name("box").unwrap()).unwrap();
        let input = openrust::console::CommandDefinition::from_signature("table")
            .unwrap()
            .parse(&[])
            .unwrap();
        let mut output = Output::default();
        let mut io = input.io(&mut output);
        io.table(["A", "B"], [["one", "two"]]).unwrap();
        let expected = cases()
            .into_iter()
            .find(|c| {
                c["decorated"] == false
                    && c["ops"]
                        == serde_json::json!([
                            ["named", "default"],
                            ["mutate", "default", "~"],
                            ["render"]
                        ])
            })
            .unwrap()["trace"][2]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(output.stdout, expected);
        let mut output = Output::default();
        let mut io = input.io(&mut output);
        io.table_named(["A", "B"], [["one", "two"]], "custom")
            .unwrap();
        let mut expected = Output::default();
        let mut expected_io = input.io(&mut expected);
        expected_io
            .table_with(
                ["A", "B"],
                [["one", "two"]],
                TableStyle::from_name("box").unwrap(),
            )
            .unwrap();
        assert_eq!(output.stdout, expected.stdout);
        let mut output = Output::default();
        input
            .io(&mut output)
            .table_with_named_columns(["A", "B"], [["one", "two"]], "custom", [(0, "compact")])
            .unwrap();
        let mut expected = Output::default();
        input
            .io(&mut expected)
            .table_with_columns(
                ["A", "B"],
                [["one", "two"]],
                TableStyle::from_name("box").unwrap(),
                [(0, TableStyle::from_name("compact").unwrap())],
            )
            .unwrap();
        assert_eq!(output.stdout, expected.stdout);
        let mut output = Output::default();
        assert!(
            input
                .io(&mut output)
                .table_named(["A"], [["one"]], "missing")
                .is_err()
        );
        assert!(output.stdout.is_empty());
    })
    .await;
}
struct MutatingFormatter {
    base: openrust::console::OutputFormatter,
    style: TableStyle,
    trigger: String,
    fired: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Clone for MutatingFormatter {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            style: TableStyle::from(&self.style),
            trigger: self.trigger.clone(),
            fired: self.fired.clone(),
        }
    }
}
impl openrust::console::OutputFormatterInterface for MutatingFormatter {
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
        if message.unwrap_or("").contains(&self.trigger) && !self.fired.swap(true, Ordering::SeqCst)
        {
            TableStyle::from(&self.style)
                .set_horizontal_border_chars("~", None)
                .set_padding_char(".")
                .unwrap()
                .set_pad_type(TablePadding::Left);
        }
        Ok(Some(self.base.format_optional(message)?))
    }
}
#[test]
fn formatter_callbacks_mutate_live_styles_without_locking_user_code() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-definitions-callback-oracle.json"
    ))
    .unwrap();
    for case in cases {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let expected = case["output"].as_str().unwrap().to_owned();
        let worker = std::thread::spawn(move || {
            let style = TableStyle::default();
            let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let mut output = Output::default();
            output.set_formatter(std::sync::Arc::new(MutatingFormatter {
                base: openrust::console::OutputFormatter::new(false),
                style: TableStyle::from(&style),
                trigger: case["trigger"].as_str().unwrap().to_owned(),
                fired: fired.clone(),
            }));
            let input = openrust::console::CommandDefinition::from_signature("table")
                .unwrap()
                .parse(&[])
                .unwrap();
            let mut io = input.io(&mut output);
            let mut table = Table::new(io.get_output()).unwrap();
            table
                .set_headers(["A", "B"])
                .set_rows([["one", "two"]])
                .set_style(&style)
                .render()
                .unwrap();
            assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
            tx.send(output.stdout).unwrap();
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("格式化回调不应被样式锁阻塞"),
            expected
        );
        worker.join().unwrap();
    }
}
