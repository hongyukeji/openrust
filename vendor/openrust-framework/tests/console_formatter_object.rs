use openrust::console::OutputFormatter;
#[test]
fn fixed_source_wrapping() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/formatter-object-source.json")).unwrap();
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let formatter = OutputFormatter::new(case["decorated"].as_bool().unwrap());
        let actual =
            formatter.format_and_wrap(case["text"].as_str(), case["width"].as_i64().unwrap());
        assert_eq!(
            actual.is_err(),
            case["error"].as_bool().unwrap(),
            "case {index}: {case} {actual:?}"
        );
        if let Ok(actual) = actual {
            assert_eq!(
                actual,
                case["output"].as_str().unwrap(),
                "case {index}: {case}"
            );
        }
    }
}
use openrust::console::{
    ConsoleError, ConsoleFormatError, FormatterHandle, FormatterStyle, Input, Output, OutputFormat,
    OutputFormatterInterface, OutputFormatterStyle, WrappableOutputFormatterInterface,
};
use openrust::{
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Custom {
    inner: OutputFormatter,
    calls: Arc<Mutex<Vec<Option<String>>>>,
}
impl OutputFormatterInterface for Custom {
    fn set_decorated(&self, v: bool) {
        self.inner.set_decorated(v);
    }
    fn is_decorated(&self) -> bool {
        self.inner.is_decorated()
    }
    fn set_style(&self, n: &str, v: FormatterStyle) {
        self.inner.set_style(n, v);
    }
    fn has_style(&self, n: &str) -> bool {
        self.inner.has_style(n)
    }
    fn get_style(&self, n: &str) -> Result<FormatterStyle, ConsoleFormatError> {
        self.inner.get_style(n)
    }
    fn format(&self, m: Option<&str>) -> Result<Option<String>, ConsoleFormatError> {
        self.calls.lock().unwrap().push(m.map(str::to_owned));
        if m == Some("error") {
            return Err(ConsoleFormatError::from_source(std::io::Error::other(
                "格式器消费者错误",
            )));
        }
        if m == Some("omit") {
            return Ok(None);
        }
        Ok(Some(format!("[{}]", self.inner.format_optional(m)?)))
    }
    fn as_wrappable(&self) -> Option<&dyn WrappableOutputFormatterInterface> {
        Some(self)
    }
}
impl WrappableOutputFormatterInterface for Custom {
    fn format_and_wrap(&self, m: Option<&str>, w: i64) -> Result<String, ConsoleFormatError> {
        self.inner.format_and_wrap(m, w)
    }
}
#[test]
fn public_formatter_shares_style_objects_and_deep_clones_palette_only() {
    let f = OutputFormatter::new(true);
    let a: FormatterStyle = Arc::new(OutputFormatterStyle::new(Some("red"), None, &[]).unwrap());
    f.set_style("CuStOm", a.clone());
    assert!(f.has_style("CUSTOM"));
    assert!(Arc::ptr_eq(&f.get_style("custom").unwrap(), &a));
    assert!(f.get_style("missing").is_err());
    assert_eq!(f.format("<custom>a").unwrap(), "\x1b[31ma\x1b[39m");
    let copy = f.clone();
    assert!(!Arc::ptr_eq(&copy.get_style("custom").unwrap(), &a));
    a.set_foreground(Some("green")).unwrap();
    assert_eq!(f.format("b").unwrap(), "\x1b[32mb\x1b[39m");
    assert_eq!(copy.format("c").unwrap(), "\x1b[32mc\x1b[39m");
    assert_eq!(
        copy.get_style("custom").unwrap().apply("d").unwrap(),
        "\x1b[31md\x1b[39m"
    );
    let stack = f.get_style_stack();
    stack.reset();
    assert_eq!(f.format("e").unwrap(), "e");
    stack.push(a.clone());
    assert!(Arc::ptr_eq(&stack.get_current(), &a));
    assert!(Arc::ptr_eq(&stack.pop().unwrap(), &a));
    let interface: &dyn OutputFormatterInterface = &f;
    assert_eq!(interface.format(None).unwrap(), Some(String::new()));
    assert_eq!(
        interface
            .as_wrappable()
            .unwrap()
            .format_and_wrap(Some("abc def"), 3)
            .unwrap(),
        "abc\ndef"
    );
}
#[test]
fn replaced_formatter_is_shared_and_raw_bypasses_it() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let formatter: FormatterHandle = Arc::new(Custom {
        inner: OutputFormatter::new(false),
        calls: calls.clone(),
    });
    let mut out = Output::default();
    out.set_formatter(formatter.clone());
    assert!(Arc::ptr_eq(&out.get_formatter(), &formatter));
    assert_eq!(
        out.get_formatter().format(Some("<info>x</info>")).unwrap(),
        Some("[x]".into())
    );
    assert_eq!(out.get_formatter().format(Some("omit")).unwrap(), None);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![Some("<info>x</info>".into()), Some("omit".into())]
    );
    out.get_formatter().set_decorated(true);
    assert!(out.is_decorated());
    assert_eq!(
        out.get_formatter().format(Some("<info>y</info>")).unwrap(),
        Some("[\x1b[32my\x1b[39m]".into())
    );
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with("formatter:custom", |input: &Input, output: &mut Output| {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let custom: FormatterHandle = Arc::new(Custom {
            inner: OutputFormatter::new(false),
            calls: calls.clone(),
        });
        let mut io = input.io(output);
        let style = io.get_output();
        style.set_formatter(custom.clone());
        assert!(Arc::ptr_eq(&style.get_formatter(), &custom));
        style.write("<info>first</info>", false, OutputFormat::Normal)?;
        {
            let mut bar = style.create_progress_bar(1)?;
            let mut target = bar.get_output();
            assert!(Arc::ptr_eq(&target.get_formatter(), &custom));
            target.write("second", false, OutputFormat::Normal)?;
            target.set_decorated(true);
            assert!(custom.is_decorated());
        }
        style.write("third", false, OutputFormat::Raw)?;
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .any(|v| v.as_deref() == Some("second"))
        );
        Ok(0)
    })?;
    Artisan::command_with(
        "formatter:selected",
        |input: &Input, output: &mut Output| {
            let terminal = output.streams_to_terminal();
            let custom: FormatterHandle = Arc::new(Custom {
                inner: OutputFormatter::new(false),
                calls: Arc::new(Mutex::new(Vec::new())),
            });
            let mut io = input.io(output);
            let style = io.get_output();
            style.set_formatter(custom.clone());
            style.write("primary", false, OutputFormat::Normal)?;
            {
                let mut bar = style.create_progress_bar(1)?;
                let mut selected = bar.get_output();
                let replacement: FormatterHandle = Arc::new(OutputFormatter::new(false));
                selected.set_formatter(replacement.clone());
                assert!(Arc::ptr_eq(&selected.get_formatter(), &replacement));
                selected.write("<info>secondary</info>", false, OutputFormat::Normal)?;
                selected.set_decorated(true);
                assert!(replacement.is_decorated());
                assert!(!custom.is_decorated());
            }
            assert_eq!(style.is_decorated(), !terminal);
            style.write("<info>after</info>", false, OutputFormat::Normal)?;
            Ok(0)
        },
    )?;
    Artisan::command_with("formatter:failure", |input: &Input, output: &mut Output| {
        let custom: FormatterHandle = Arc::new(Custom {
            inner: OutputFormatter::new(false),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let mut io = input.io(output);
        let style = io.get_output();
        style.set_formatter(custom);
        style.write("error", false, OutputFormat::Normal)?;
        Ok(0)
    })?;
    Ok(())
}
#[tokio::test]
async fn public_formatter_runs_through_artisan_and_selected_progress_stream() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let mut output = Output::default();
        let status = Artisan::call("formatter:custom", &[], &mut output)
            .await
            .unwrap();
        assert_eq!(status, 0);
        assert_eq!(output.stdout, "[first][second]third");
    })
    .await;
}
#[test]
fn fixed_source_escape_and_trailing_backslash() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/formatter-escape-source.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        assert_eq!(
            OutputFormatter::escape(text),
            case["escaped"].as_str().unwrap(),
            "{case}"
        );
        assert_eq!(
            OutputFormatter::escape_trailing_backslash(text),
            case["trailing"].as_str().unwrap(),
            "{case}"
        );
    }
}
#[test]
#[ignore = "由实际终端输出父用例驱动"]
fn formatter_cli_fixture() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::path::PathBuf::from(
        std::env::var_os("FORMATTER_ROOT").unwrap(),
    ))
    .with_routing(Routing::default().commands(register))
    .create()
    .unwrap();
    rt.block_on(app.boot()).unwrap();
    let command = std::env::var("FORMATTER_COMMAND").unwrap_or_else(|_| "formatter:custom".into());
    let code = rt
        .block_on(app.handle_command(&[command.into(), "--no-ansi".into()]))
        .unwrap();
    std::process::exit(code);
}
#[test]
fn actual_artisan_formatter_replacement_shares_stdout_and_stderr() {
    let root = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "formatter_cli_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("FORMATTER_ROOT", root.path())
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(child.stderr, b"[second]");
    assert_eq!(
        String::from_utf8(child.stdout)
            .unwrap()
            .strip_prefix("\nrunning 1 test\n")
            .unwrap(),
        "[first]third"
    );
}
#[derive(Clone)]
struct ReentrantStyle {
    owner: std::sync::Weak<OutputFormatter>,
    inner: OutputFormatterStyle,
}
impl openrust::console::OutputFormatterStyleInterface for ReentrantStyle {
    fn set_foreground(&self, v: Option<&str>) -> Result<(), ConsoleFormatError> {
        self.inner.set_foreground(v)
    }
    fn set_background(&self, v: Option<&str>) -> Result<(), ConsoleFormatError> {
        self.inner.set_background(v)
    }
    fn set_option(&self, v: &str) -> Result<(), ConsoleFormatError> {
        self.inner.set_option(v)
    }
    fn unset_option(&self, v: &str) -> Result<(), ConsoleFormatError> {
        self.inner.unset_option(v)
    }
    fn set_options(&self, v: &[&str]) -> Result<(), ConsoleFormatError> {
        self.inner.set_options(v)
    }
    fn apply(&self, v: &str) -> Result<String, ConsoleFormatError> {
        let owner = self.owner.upgrade().unwrap();
        owner.get_style_stack().reset();
        owner.set_style("added", Arc::new(OutputFormatterStyle::default()));
        assert_eq!(owner.format("nested")?, "nested");
        self.inner.apply(v)
    }
}
#[test]
fn user_style_can_reenter_formatter_and_reset_the_same_stack_without_a_data_lock() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let f = Arc::new(OutputFormatter::new(true));
        let style: FormatterStyle = Arc::new(ReentrantStyle {
            owner: Arc::downgrade(&f),
            inner: OutputFormatterStyle::default(),
        });
        f.set_style("callback", style);
        assert_eq!(f.format("<callback>outer</>").unwrap(), "outer");
        assert!(f.has_style("added"));
        tx.send(()).unwrap();
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .expect("用户样式重入不得阻塞");
}

fn formatter_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    for path in ["/styles", "/styles/error"] {
        let error = path.ends_with("error");
        Route::get(path, move || async move {
            let mut out = Output::default();
            Artisan::call(
                if error {
                    "formatter:failure"
                } else {
                    "formatter:custom"
                },
                &[],
                &mut out,
            )
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                serde_json::json!({"styled":out.stdout}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_uses_the_replaced_formatter_and_recovers_after_failure() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register).web(formatter_routes))
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
    for (path, status) in [("/styles", 200), ("/styles/error", 500), ("/styles", 200)] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert_eq!(
                response.json::<serde_json::Value>().await.unwrap()["styled"],
                "[first][second]third"
            );
        }
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn selected_stream_replacement_and_mutation_match_actual_source_objects() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/formatter-selected-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("formatter:selected", &[], &mut output)
                .await
                .unwrap(),
            0
        );
        assert_eq!(output.stdout, rows[1]["stdout"].as_str().unwrap());
        assert_eq!(output.stderr, rows[1]["stderr"].as_str().unwrap());
    })
    .await;
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "formatter_cli_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("FORMATTER_ROOT", root.path())
        .env("FORMATTER_COMMAND", "formatter:selected")
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(
        String::from_utf8(child.stdout)
            .unwrap()
            .strip_prefix("\nrunning 1 test\n")
            .unwrap(),
        rows[0]["stdout"].as_str().unwrap()
    );
    assert_eq!(
        String::from_utf8(child.stderr).unwrap(),
        rows[0]["stderr"].as_str().unwrap()
    );
}
