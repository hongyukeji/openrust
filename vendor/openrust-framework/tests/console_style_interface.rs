#[path = "support/style_interface.rs"]
mod style_interface_consumer;
use openrust::console::{
    AsOutput, ChoiceKey, CommandDefinition, ConsoleError, ConsoleInputStream, Output,
    OutputReference, OutputStyle, OutputVerbosity, ProgressBar, QuestionCallback, StyleInterface,
    contracts::NewLineAware,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use style_interface_consumer::exercise;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/style-interface-oracle.json"
    ))
    .unwrap()
}
fn parents(output: &mut OutputReference<'_>) -> Vec<Value> {
    let mut result = Vec::new();
    if let Some(view) = output.as_new_line_aware() {
        result.push(json!([view.new_lines_written(), view.new_line_written()]));
        result.extend(parents(&mut output.get_output().unwrap()));
    }
    result
}
async fn run_case(style: &mut OutputStyle<'_>, c: &Value, dynamic: bool) -> Value {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let steps = if dynamic {
        exercise(style as &mut dyn StyleInterface, c, trace.clone()).await
    } else {
        exercise(style, c, trace.clone()).await
    };
    let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
    counts.extend(parents(style.get_output()));
    counts.reverse();
    json!({"steps":steps,"output":style.get_output().stdout,"counts":counts,"trace":*trace.lock().unwrap()})
}
fn expected(c: &Value) -> Value {
    let steps: Vec<Value> = c["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| json!({"method":v["method"],"value":v["value"],"failed":v["error"].is_string()}))
        .collect();
    json!({"steps":steps,"output":c["output"],"counts":c["counts"],"trace":c["trace"]})
}
#[test]
fn all_eighteen_interface_entries_are_actual_generic_and_dynamic_consumers() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "style_interface_driver",
            "--ignored",
            "--nocapture",
        ])
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
#[ignore = "由父消费者隔离尺寸与应用进度定义"]
fn style_interface_driver() {
    use openrust::{
        foundation::Application,
        support::facades::{App, Facade as _},
    };
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into()).create().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(app.boot()).unwrap();
    rt.block_on(App::set_facade_application(Some(&app), async {
        for name in [
            "normal",
            "verbose",
            "very_verbose",
            "debug",
            "normal_nomax",
            "verbose_nomax",
            "very_verbose_nomax",
            "debug_nomax",
        ] {
            ProgressBar::set_format_definition(name, "%current%/%max%").unwrap();
        }
        let mut failures = Vec::new();
        for dynamic in [false, true] {
            for (index, c) in cases().iter().enumerate() {
                let mut input = CommandDefinition::from_signature("style-interface")
                    .unwrap()
                    .parse(&[])
                    .unwrap();
                input.set_interactive(c["interactive"].as_bool().unwrap());
                input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                    std::io::Cursor::new(b"Ada\nsample\n\n\n".to_vec()),
                ))));
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
                    run_case(&mut second, c, dynamic).await
                } else {
                    run_case(&mut first, c, dynamic).await
                };
                if actual != expected(c) {
                    failures.push(format!(
                        "case{index}/{dynamic}: actual{actual:?},expected{:?}",
                        expected(c)
                    ));
                }
                assert_eq!(
                    input.is_interactive(),
                    c["interactive_after"].as_bool().unwrap(),
                    "interactive{index}/{dynamic}"
                );
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }));
}

fn configure_formats() -> Result<(), ConsoleError> {
    for name in [
        "normal",
        "verbose",
        "very_verbose",
        "debug",
        "normal_nomax",
        "verbose_nomax",
        "very_verbose_nomax",
        "debug_nomax",
    ] {
        ProgressBar::set_format_definition(name, "%current%/%max%")?;
    }
    Ok(())
}
fn register() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:interface {--fail}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                configure_formats()?;
                let mut style = OutputStyle::new(input, output);
                let dynamic: &mut dyn openrust::console::style::StyleInterface = &mut style;
                if input.option("fail").and_then(|value| value.as_bool()) == Some(true) {
                    dynamic.title("<fg=bogus>Title</>")?;
                    return Ok(0);
                }
                let trace = Arc::new(Mutex::new(Vec::new()));
                let steps = exercise(dynamic, &json!({"variant":0}), trace.clone()).await;
                if std::env::var_os("STYLE_INTERFACE_CASE").is_some() {
                    eprintln!(
                        "STYLE_INTERFACE_META{}",
                        json!({"steps":steps,"trace":*trace.lock().unwrap(),"counts":[[style.new_lines_written(),style.new_line_written()]]})
                    );
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn console_cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/style-interface-console-oracle.json"
    ))
    .unwrap()
}
#[test]
fn actual_artisan_dynamic_style_interface_matches_source_console_streams() {
    for c in console_cases() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "style_interface_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("STYLE_INTERFACE_CASE", c.to_string())
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(
            stdout.split_once("STYLE_INTERFACE_BEGIN").unwrap().1,
            c["stdout"].as_str().unwrap()
        );
        let stderr = String::from_utf8(child.stderr).unwrap();
        let (error_stream, meta) = stderr.rsplit_once("STYLE_INTERFACE_META").unwrap();
        assert_eq!(error_stream, c["stderr"].as_str().unwrap());
        let metadata: Value = serde_json::from_str(meta).unwrap();
        assert_eq!(metadata["counts"], c["metadata"]["counts"]);
        assert_eq!(metadata["trace"], c["metadata"]["trace"]);
        assert_eq!(metadata["steps"], expected(&c["metadata"])["steps"]);
    }
}
#[test]
fn actual_artisan_dynamic_interface_propagates_failure_exit() {
    for decorated in [false, true] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "style_interface_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env(
                "STYLE_INTERFACE_CASE",
                json!({"decorated":decorated,"level":32,"fatal":true}).to_string(),
            )
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(1),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
}
#[test]
#[ignore = "由父消费者实际启动独立Artisan并核对退出码"]
fn style_interface_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let c: Value = serde_json::from_str(&std::env::var("STYLE_INTERFACE_CASE").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let mut args = vec![
        "styles:interface".into(),
        "--no-interaction".into(),
        if c["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    if c["level"] == 16 {
        args.push("--quiet".into());
    }
    if c["fatal"] == true {
        args.push("--fail".into());
    }
    print!("STYLE_INTERFACE_BEGIN");
    std::io::stdout().flush().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let status = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(status);
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, fail) in [
        ("/style-interface", false),
        ("/style-interface/error", true),
    ] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args = vec!["--no-ansi".into(), "--no-interaction".into()];
            if fail {
                args.push("--fail".into());
            }
            openrust::support::facades::Artisan::call("styles:interface", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
        })?;
    }
    Ok(())
}
fn expected_http() -> Value {
    let c = cases()
        .into_iter()
        .find(|c| {
            c["variant"] == 0
                && c["interactive"] == false
                && c["depth"] == 1
                && c["section"] == false
                && c["decorated"] == false
                && c["level"] == 32
        })
        .unwrap();
    json!({"output":c["output"]})
}
#[tokio::test]
async fn real_http_dynamic_interface_errors_recover_and_remain_application_scoped() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing},
    };
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
    for (path, status) in [
        ("/style-interface", 200),
        ("/style-interface/error", 500),
        ("/style-interface", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(
            response.headers()[reqwest::header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let body: Value = response.json().await.unwrap();
        if status == 200 {
            assert_eq!(body, expected_http());
        } else {
            assert!(body.is_object());
        }
    }
    let mut tasks = Vec::new();
    for _ in 0..12 {
        tasks.push(tokio::spawn(async move {
            let body: Value = reqwest::get(format!("http://{address}/style-interface"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(body, expected_http());
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

fn register_interface_live() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:interface-live",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut reading = input.clone();
                reading.set_interactive(true);
                let metadata = {
                    let mut style = OutputStyle::new(&reading, output);
                    let dynamic: &mut dyn StyleInterface = &mut style;
                    let validator = QuestionCallback::sync(|value| {
                        if value == "bad" {
                            return Err(ConsoleError::new("Try again"));
                        }
                        Ok(json!({"name":value.as_str().unwrap_or_default().to_uppercase()}))
                    });
                    let name = dynamic.ask("Name", Some("Guest"), Some(validator)).await?;
                    let secret = dynamic
                        .ask_hidden(
                            "Password",
                            Some(QuestionCallback::sync(|value| {
                                Ok(json!({"length":value.as_str().unwrap_or_default().len()}))
                            })),
                        )
                        .await?;
                    let confirm = dynamic.confirm("Continue", true).await?;
                    let choice = dynamic
                        .choice(
                            "Color",
                            vec![
                                (ChoiceKey::from(0_i64), json!("red")),
                                (ChoiceKey::from(1_i64), json!("green")),
                            ],
                            Some(json!("green")),
                        )
                        .await?;
                    json!({"name":name,"secret":secret,"confirm":confirm,"choice":choice})
                };
                output
                    .stderr
                    .push_str(&format!("STYLE_QUESTIONS_META:{metadata}\n"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由真实PTY父消费者验收动态Trait隐藏输入与取消"]
fn style_interface_live_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let app = Application::configure(std::env::var_os("STYLE_QUESTIONS_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_interface_live))
        .create()
        .unwrap();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&["styles:interface-live".into(), "--no-ansi".into()]))
        .unwrap();
    if let Some(path) = std::env::var_os("STYLE_QUESTIONS_RELEASE") {
        eprintln!("STYLE_QUESTIONS_STATUS:{status}");
        let path = std::path::PathBuf::from(path);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    std::process::exit(status);
}
