use openrust::{
    console::{
        ConsoleError, ConsoleFormatError, FormatterStyle, Input, Output, OutputFormat,
        OutputFormatterStyle, OutputFormatterStyleInterface, OutputFormatterStyleStack,
    },
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::Value;
use std::sync::Arc;
fn modes() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/formatter-style-oracle.json"
    ))
    .unwrap()
}
fn verify(case: &Value) -> Result<String, ConsoleError> {
    let ctor = case["ctor"].as_array().unwrap();
    let options: Vec<&str> = ctor[2]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let style = OutputFormatterStyle::new(ctor[0].as_str(), ctor[1].as_str(), &options);
    assert_eq!(
        style.is_err(),
        case["constructor_error"].as_bool().unwrap(),
        "{case}"
    );
    let Ok(concrete) = style else {
        return Ok(String::new());
    };
    let concrete = Arc::new(concrete);
    let style: FormatterStyle = concrete.clone();
    let text = case["text"].as_str().unwrap();
    let initial = style.apply(text).unwrap();
    assert_eq!(initial, case["initial"].as_str().unwrap(), "{case}");
    let mut output = initial;
    for (index, op) in case["ops"].as_array().unwrap().iter().enumerate() {
        let result = match op[0].as_str().unwrap() {
            "setForeground" => style.set_foreground(op[1].as_str()),
            "setBackground" => style.set_background(op[1].as_str()),
            "setOption" => style.set_option(op[1].as_str().unwrap()),
            "unsetOption" => style.unset_option(op[1].as_str().unwrap()),
            "setOptions" => {
                let values: Vec<_> = op[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                style.set_options(&values)
            }
            "setHref" => {
                concrete.set_href(op[1].as_str().unwrap());
                Ok(())
            }
            _ => panic!("未知样式操作"),
        };
        assert_eq!(
            result.is_err(),
            case["trace"][index]["error"].as_bool().unwrap(),
            "{case},op={op}"
        );
        let rendered = style.apply(text).unwrap();
        assert_eq!(
            rendered,
            case["trace"][index]["output"].as_str().unwrap(),
            "{case},op={op}"
        );
        output.push_str(&rendered);
    }
    Ok(output)
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "formatter:style {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let text = verify(&case)?;
            input
                .io(output)
                .get_output()
                .write(text.as_str(), false, OutputFormat::Raw)?;
            Ok(0)
        },
    )?;
    Artisan::command_with("formatter:error", |_: &Input, _: &mut Output| {
        let style: FormatterStyle = Arc::new(Quoted {
            inner: Arc::new(OutputFormatterStyle::default()),
        });
        style.apply("failure")?;
        Ok(0)
    })?;
    Ok(())
}
#[test]
#[ignore = "由终端环境隔离的实际Artisan子进程驱动"]
fn styles_cli_fixture() {
    let mode: Value = serde_json::from_str(&std::env::var("STYLE_MODE").unwrap()).unwrap();
    let root = std::env::var("STYLE_ROOT").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    if mode["failure"] == true {
        let app = Application::configure(std::path::PathBuf::from(&root))
            .with_routing(Routing::default().commands(register))
            .create()
            .unwrap();
        let code = runtime
            .block_on(app.handle_command(&["formatter:error".into(), "--no-ansi".into()]))
            .unwrap();
        std::process::exit(code);
    }
    for case in mode["cases"].as_array().unwrap() {
        let app = Application::configure(std::path::PathBuf::from(&root))
            .with_routing(Routing::default().commands(register))
            .create()
            .unwrap();
        let args = vec![
            "formatter:style".into(),
            case.to_string().into(),
            "--no-ansi".into(),
        ];
        let code = runtime.block_on(app.handle_command(&args)).unwrap();
        assert_eq!(code, 0);
    }
    let stack_modes: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/formatter-stack-oracle.json"
    ))
    .unwrap();
    let stack_mode = stack_modes
        .iter()
        .find(|item| item["name"] == mode["name"])
        .unwrap();
    for case in stack_mode["cases"].as_array().unwrap() {
        verify_stack(case);
    }
    std::process::exit(0);
}
#[test]
fn public_styles_match_fixed_source_in_real_terminal_environment_modes() {
    let root = tempfile::tempdir().unwrap();
    for mode in modes() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "styles_cli_fixture", "--ignored", "--nocapture"])
            .env("STYLE_ROOT", root.path())
            .env("STYLE_MODE", mode.to_string());
        for key in [
            "TERM",
            "COLORTERM",
            "TERMINAL_EMULATOR",
            "KONSOLE_VERSION",
            "IDEA_INITIAL_DIRECTORY",
        ] {
            command.env_remove(key);
        }
        for (key, value) in mode["env"].as_object().unwrap() {
            command.env(key, value.as_str().unwrap());
        }
        let child = command.output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "{}: {}\n{}",
            mode["name"],
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(child.stderr, b"");
        let expected: String = mode["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["constructor_error"] == false)
            .flat_map(|case| {
                std::iter::once(case["initial"].as_str().unwrap()).chain(
                    case["trace"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| row["output"].as_str().unwrap()),
                )
            })
            .collect();
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(stdout.strip_prefix("\nrunning 1 test\n").unwrap(), expected);
    }
}
#[test]
fn retained_style_references_stack_pop_and_deep_style_clones_are_real_public_consumers() {
    let first = Arc::new(OutputFormatterStyle::new(Some("white"), Some("black"), &[]).unwrap());
    let second = Arc::new(OutputFormatterStyle::new(Some("yellow"), Some("blue"), &[]).unwrap());
    let third = Arc::new(OutputFormatterStyle::new(Some("green"), Some("red"), &[]).unwrap());
    let first: FormatterStyle = first;
    let second: FormatterStyle = second;
    let third: FormatterStyle = third;
    let mut stack = OutputFormatterStyleStack::default();
    let empty = stack.get_empty_style();
    assert!(Arc::ptr_eq(&empty, &stack.pop().unwrap()));
    stack.push(first.clone());
    stack.push(second.clone());
    stack.push(third.clone());
    assert!(Arc::ptr_eq(&third, &stack.get_current()));
    let mut cloned = stack.clone();
    assert!(Arc::ptr_eq(
        &second,
        &stack.pop_with_style(&second).unwrap()
    ));
    assert!(Arc::ptr_eq(&first, &stack.pop().unwrap()));
    assert!(Arc::ptr_eq(&empty, &stack.pop().unwrap()));
    let equivalent: FormatterStyle =
        Arc::new(OutputFormatterStyle::new(Some("yellow"), Some("blue"), &[]).unwrap());
    assert!(Arc::ptr_eq(
        &second,
        &cloned.pop_with_style(&equivalent).unwrap()
    ));
    assert!(cloned.pop_with_style(&third).is_err());
    first.set_foreground(Some("red")).unwrap();
    assert_eq!(
        cloned.get_current().apply("x").unwrap(),
        "\x1b[31;40mx\x1b[39;49m"
    );
    let copied = dyn_clone::clone_box(first.as_ref());
    copied.set_foreground(Some("green")).unwrap();
    assert_ne!(copied.apply("x").unwrap(), first.apply("x").unwrap());
    cloned.set_empty_style(second.clone()).reset();
    assert!(Arc::ptr_eq(&second, &cloned.get_current()));
    assert!(Arc::ptr_eq(&second, &cloned.get_empty_style()));
}
#[derive(Clone)]
struct Quoted {
    inner: Arc<OutputFormatterStyle>,
}
impl OutputFormatterStyleInterface for Quoted {
    fn set_foreground(&self, color: Option<&str>) -> Result<(), ConsoleFormatError> {
        self.inner.set_foreground(color)
    }
    fn set_background(&self, color: Option<&str>) -> Result<(), ConsoleFormatError> {
        self.inner.set_background(color)
    }
    fn set_option(&self, option: &str) -> Result<(), ConsoleFormatError> {
        self.inner.set_option(option)
    }
    fn unset_option(&self, option: &str) -> Result<(), ConsoleFormatError> {
        self.inner.unset_option(option)
    }
    fn set_options(&self, options: &[&str]) -> Result<(), ConsoleFormatError> {
        self.inner.set_options(options)
    }
    fn apply(&self, text: &str) -> Result<String, ConsoleFormatError> {
        if text == "failure" {
            return Err(ConsoleFormatError::from_source(std::io::Error::other(
                "custom style failed",
            )));
        }
        Ok(format!("[{}]", self.inner.apply(text)?))
    }
}
#[test]
fn custom_style_trait_objects_preserve_apply_errors_and_clone_the_declared_implementation() {
    let style: FormatterStyle = Arc::new(Quoted {
        inner: Arc::new(OutputFormatterStyle::default()),
    });
    style.set_foreground(Some("green")).unwrap();
    style.set_background(None).unwrap();
    style.set_options(&["bold"]).unwrap();
    style.set_option("underscore").unwrap();
    style.unset_option("bold").unwrap();
    assert_eq!(style.apply("x").unwrap(), "[\x1b[32;4mx\x1b[39;24m]");
    let copied = dyn_clone::clone_box(style.as_ref());
    assert_eq!(copied.apply("x").unwrap(), style.apply("x").unwrap());
    let error = copied.apply("failure").unwrap_err();
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .is::<std::io::Error>()
    );
    let mut stack = OutputFormatterStyleStack::new(Some(style.clone()));
    stack.push(style.clone());
    assert!(Arc::ptr_eq(&style, &stack.pop_with_style(&style).unwrap()));
}
#[tokio::test]
async fn styles_also_run_inside_the_registered_application_scope() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let mode = &modes()[0];
    App::set_facade_application(Some(&app), async {
        for case in mode["cases"].as_array().unwrap().iter().take(40) {
            let mut output = Output::default();
            Artisan::call("formatter:style", &[case.to_string().into()], &mut output)
                .await
                .unwrap();
        }
    })
    .await;
}
fn verify_stack(case: &Value) {
    let a = Arc::new(OutputFormatterStyle::new(Some("white"), Some("black"), &[]).unwrap());
    let b = Arc::new(OutputFormatterStyle::new(Some("yellow"), Some("blue"), &[]).unwrap());
    let c = Arc::new(OutputFormatterStyle::new(Some("green"), Some("red"), &[]).unwrap());
    let d = Arc::new(OutputFormatterStyle::new(Some("yellow"), Some("blue"), &[]).unwrap());
    let e = Arc::new(OutputFormatterStyle::default());
    e.set_href("https://example.test/a");
    let f = Arc::new(OutputFormatterStyle::default());
    f.set_href("https://example.test/b");
    let styles: Vec<FormatterStyle> = vec![a, b, c, d, e, f];
    let mut stack = OutputFormatterStyleStack::default();
    let empty = stack.get_empty_style();
    let mut cloned: Option<OutputFormatterStyleStack> = None;
    let identify = |style: &FormatterStyle| -> Value {
        if Arc::ptr_eq(style, &empty) {
            return Value::String("default".into());
        }
        styles
            .iter()
            .position(|item| Arc::ptr_eq(item, style))
            .map(|id| Value::from(id as u64))
            .unwrap_or_else(|| Value::String("unknown".into()))
    };
    for (index, op) in case["ops"].as_array().unwrap().iter().enumerate() {
        let result: Result<Option<FormatterStyle>, ConsoleFormatError> =
            match op[0].as_str().unwrap() {
                "push" => {
                    stack.push(styles[op[1].as_u64().unwrap() as usize].clone());
                    Ok(None)
                }
                "pop" => match op[1].as_u64() {
                    Some(id) => stack.pop_with_style(&styles[id as usize]).map(Some),
                    None => stack.pop().map(Some),
                },
                "current" => Ok(Some(stack.get_current())),
                "empty" => Ok(Some(stack.get_empty_style())),
                "setEmpty" => {
                    stack.set_empty_style(styles[op[1].as_u64().unwrap() as usize].clone());
                    Ok(None)
                }
                "reset" => {
                    let contract: &mut dyn openrust::contracts::service::ResetInterface =
                        &mut stack;
                    contract.reset();
                    Ok(None)
                }
                "foreground" => styles[op[1].as_u64().unwrap() as usize]
                    .set_foreground(op[2].as_str())
                    .map(|()| None),
                "clone" => {
                    cloned = Some(stack.clone());
                    Ok(None)
                }
                "cloneCurrent" => Ok(Some(cloned.as_ref().unwrap().get_current())),
                "clonePop" => cloned.as_mut().unwrap().pop().map(Some),
                _ => panic!("未知样式栈操作"),
            };
        let error = result.is_err();
        let returned = result
            .ok()
            .flatten()
            .as_ref()
            .map(identify)
            .unwrap_or(Value::Null);
        let trace = serde_json::json!({"error":error,"returned":returned,"output":stack.get_current().apply("x").unwrap(),"current":identify(&stack.get_current()),"clone_output":cloned.as_ref().map(|stack|stack.get_current().apply("x").unwrap())});
        assert_eq!(trace, case["trace"][index], "{case},op={op}");
    }
}
#[test]
fn iterator_and_string_adapters_evaluate_user_code_before_borrowing_style_state() {
    let style = OutputFormatterStyle::default();
    style
        .set_options(std::iter::once("bold").inspect(|_| {
            style.set_foreground(Some("green")).unwrap();
        }))
        .unwrap();
    struct Url<'a>(&'a OutputFormatterStyle);
    impl From<Url<'_>> for String {
        fn from(url: Url<'_>) -> String {
            url.0.set_background(Some("black")).unwrap();
            "https://example.test".into()
        }
    }
    style.set_href(Url(&style));
    let text = style.apply("ready").unwrap();
    assert!(text.contains("ready"));
    assert!(text.starts_with("\x1b[32;40;1m"));
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    for (path, error) in [("/styles", false), ("/styles/error", true)] {
        Route::get(path, move || async move {
            let mut output = Output::default();
            let result = if error {
                Artisan::call("formatter:error", &[], &mut output).await
            } else {
                Artisan::call(
                    "formatter:style",
                    &[
                        modes()[0]["cases"][0].to_string().into(),
                        "--no-ansi".into(),
                    ],
                    &mut output,
                )
                .await
            };
            result.map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                serde_json::json!({"styled":output.stdout}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_styles_custom_trait_errors_and_later_requests_use_the_public_pipeline() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
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
                response.json::<Value>().await.unwrap()["styled"],
                modes()[0]["cases"][0]["initial"]
            );
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn actual_artisan_custom_style_failures_preserve_the_error_and_exit_nonzero() {
    let root = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "styles_cli_fixture", "--ignored", "--nocapture"])
        .env("STYLE_ROOT", root.path())
        .env(
            "STYLE_MODE",
            serde_json::json!({"failure":true}).to_string(),
        )
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&child.stderr).contains("custom style failed"));
}
