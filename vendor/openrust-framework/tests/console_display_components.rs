use openrust::{
    console::{
        AsOutput, CommandDefinition, ConsoleError, ConsoleStyle, Output, OutputFormat,
        OutputOptions, OutputReference, OutputStyle, OutputVerbosity,
        view::components::{Alert, BulletList, TwoColumnDetail},
    },
    foundation::Application,
    support::facades::{App, Facade as _},
};
use serde_json::Value;
fn verbosity(value: u64) -> OutputVerbosity {
    match value {
        8 => OutputVerbosity::Silent,
        16 => OutputVerbosity::Quiet,
        64 => OutputVerbosity::Verbose,
        _ => OutputVerbosity::Normal,
    }
}
fn options(value: u64) -> OutputOptions {
    OutputOptions {
        format: match value & 7 {
            1 => OutputFormat::ExplicitNormal,
            2 => OutputFormat::Raw,
            4 => OutputFormat::Plain,
            _ => OutputFormat::Normal,
        },
        verbosity: Some(verbosity(value & !7)),
    }
}
fn invoke(
    output: &mut (impl AsOutput + ?Sized),
    case: &Value,
    base: &str,
) -> Result<(), ConsoleError> {
    let args = &case["arguments"];
    let required = case["required"].as_u64().unwrap();
    let opt = options(required);
    let formatter = case
        .get("custom_style")
        .map(|_| output.as_output().get_formatter());
    let configure = || {
        if let Some(formatter) = &formatter {
            formatter.set_style(
                case["custom_style"].as_str().unwrap(),
                std::sync::Arc::new(
                    openrust::console::OutputFormatterStyle::new(
                        Some("magenta"),
                        None,
                        &["underscore"],
                    )
                    .unwrap(),
                ),
            );
        }
    };
    match case["kind"].as_str().unwrap() {
        "Alert" => {
            let text = args[0].as_str().unwrap().replace("__BASE__", base);
            let mut component = Alert::new(output);
            configure();
            if required == 32 {
                component.render(&text)
            } else {
                component.render_with(&text, opt)
            }
        }
        "BulletList" => {
            let values: Vec<_> = args[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().replace("__BASE__", base))
                .collect();
            let mut component = BulletList::new(output);
            configure();
            if required == 32 {
                component.render(&values)
            } else {
                component.render_with(&values, opt)
            }
        }
        "TwoColumnDetail" => {
            let first = args[0].as_str().unwrap().replace("__BASE__", base);
            let second = args
                .get(1)
                .and_then(Value::as_str)
                .map(|s| s.replace("__BASE__", base));
            let mut component = TwoColumnDetail::new(output);
            configure();
            if required == 32 {
                component.render(&first, second.as_deref())
            } else {
                component.render_with(&first, second.as_deref(), opt)
            }
        }
        _ => unreachable!(),
    }
}
fn check_case(case: &Value, base: &str) {
    let mut output = Output::default();
    output.set_decorated(case["decorated"].as_bool().unwrap());
    output.set_verbosity(verbosity(case["level"].as_u64().unwrap()));
    let input = CommandDefinition::from_signature("display:show")
        .unwrap()
        .parse(&[])
        .unwrap();
    let prefix = case["prefix"].as_str().unwrap();
    let (result, count) = match case["surface"].as_str().unwrap() {
        "derived" => {
            let mut style = OutputStyle::new(&input, &mut output);
            if !prefix.is_empty() {
                style
                    .write(prefix, false, OutputOptions::default())
                    .unwrap();
            }
            let result = invoke(&mut style, case, base);
            (result, serde_json::json!(style.new_lines_written()))
        }
        "nested" => {
            let mut parent = OutputStyle::new(&input, &mut output);
            let mut style = OutputStyle::new(&input, &mut parent);
            if !prefix.is_empty() {
                style
                    .write(prefix, false, OutputOptions::default())
                    .unwrap();
            }
            let result = invoke(&mut style, case, base);
            (result, serde_json::json!(style.new_lines_written()))
        }
        "base" => {
            let mut style = ConsoleStyle::new(&input, &mut output);
            if !prefix.is_empty() {
                style
                    .write(prefix, false, OutputOptions::default())
                    .unwrap();
            }
            let result = invoke(&mut style, case, base);
            (result, Value::Null)
        }
        "raw" => {
            if !prefix.is_empty() {
                OutputReference::from(&mut output)
                    .write(prefix, false, OutputOptions::default())
                    .unwrap();
            }
            let result = invoke(&mut output, case, base);
            (result, Value::Null)
        }
        _ => unreachable!(),
    };
    assert_eq!(
        result.is_err(),
        !case["error"].is_null(),
        "{case}: {result:?}"
    );
    assert_eq!(
        output.stdout,
        case["output"].as_str().unwrap().replace("__BASE__", base),
        "{case}"
    );
    assert_eq!(count, case["new_lines"], "{case}");
    assert!(output.stderr.is_empty());
}
#[test]
#[ignore = "由父消费者设置真实COLUMNS，隔离验证终端宽度"]
fn display_width_driver() {
    let width = std::env::var("COLUMNS").unwrap().parse::<i64>().unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/display-components-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into()).create().unwrap();
    let base = root.path().display().to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut count = 0;
    rt.block_on(async {
        for case in cases.iter().filter(|case| case["width"] == width) {
            App::set_facade_application(
                if case["bound"] == true {
                    Some(&app)
                } else {
                    None
                },
                async { check_case(case, &base) },
            )
            .await;
            count += 1;
        }
    });
    assert!(count > 0);
    println!("DISPLAY_CASES_OK:{count}");
}
#[test]
fn actual_terminal_widths_and_each_public_display_type_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/display-components-oracle.json"
    ))
    .unwrap();
    let mut widths: Vec<i64> = cases
        .iter()
        .map(|case| case["width"].as_i64().unwrap())
        .collect();
    widths.sort_unstable();
    widths.dedup();
    let mut total = 0;
    for width in widths {
        let expected = cases.iter().filter(|case| case["width"] == width).count();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "display_width_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("COLUMNS", width.to_string())
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "width={width}\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stdout)
                .contains(&format!("DISPLAY_CASES_OK:{expected}"))
        );
        total += expected;
    }
    assert_eq!(total, cases.len());
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "display:show {--kind=Alert} {--invalid}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let case=std::env::var("DISPLAY_CASE").ok().map(|text|serde_json::from_str::<Value>(&text).unwrap()).unwrap_or_else(||{
            let kind=input.option("kind").and_then(|value|value.as_str().map(str::to_owned)).unwrap_or_else(||"Alert".into());
            let args=match kind.as_str(){"Alert"=>serde_json::json!(["Careful [report]"]),"BulletList"=>serde_json::json!([["First.","[second]"]]),"TwoColumnDetail"=>serde_json::json!(["File [report].","Ready"]),_=>unreachable!()};
            serde_json::json!({"kind":kind,"arguments":args,"required":32,"side":"stdout"})
        });
                let mut style = OutputStyle::new(input, output);
                if input.option("invalid").and_then(|value| value.as_bool()) == Some(true) {
                    Alert::new(&mut style).render("<options=invalid>failure</>")?;
                } else if case["side"] == "stderr" {
                    invoke(&mut style.get_error_style(), &case, "__BASE__")?;
                } else {
                    invoke(&mut style, &case, "__BASE__")?;
                }
                if std::env::var_os("DISPLAY_CASE").is_some() {
                    eprintln!("DISPLAY_META{}", style.new_lines_written());
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者运行实际Artisan并核对物理输出"]
fn display_console_driver() {
    use openrust::foundation::configuration::Routing;
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let case: Value = serde_json::from_str(&std::env::var("DISPLAY_CASE").unwrap()).unwrap();
    let mut args = vec![
        "display:show".into(),
        "--no-interaction".into(),
        if case["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    if case["level"] == 16 {
        args.push("--quiet".into());
    }
    if case["invalid"] == true {
        args.push("--invalid".into());
    }
    print!("DISPLAY_BEGIN");
    std::io::stdout().flush().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_console_streams_and_failures_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/display-console-oracle.json"
    ))
    .unwrap();
    for mut case in cases {
        case["required"] = serde_json::json!(32);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "display_console_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("DISPLAY_CASE", case.to_string())
            .env("COLUMNS", case["width"].as_i64().unwrap().to_string())
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "{case}\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert_eq!(
            stdout.split_once("DISPLAY_BEGIN").unwrap().1,
            case["stdout"].as_str().unwrap(),
            "{case}"
        );
        let stderr = String::from_utf8(result.stderr).unwrap();
        let (stream, meta) = stderr.rsplit_once("DISPLAY_META").unwrap();
        assert_eq!(stream, case["stderr"].as_str().unwrap(), "{case}");
        assert_eq!(
            meta.trim().parse::<u64>().unwrap(),
            case["new_lines"].as_u64().unwrap(),
            "{case}"
        );
    }
    let case =
        serde_json::json!({"kind":"Alert","arguments":["failure"],"required":32,"invalid":true});
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "display_console_driver",
            "--ignored",
            "--nocapture",
        ])
        .env("DISPLAY_CASE", case.to_string())
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for kind in ["Alert", "BulletList", "TwoColumnDetail"] {
        openrust::support::facades::Route::get(&format!("/display/{kind}"), move || async move {
            let mut output = Output::default();
            openrust::support::facades::Artisan::call(
                "display:show",
                &[format!("--kind={kind}").into()],
                &mut output,
            )
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                serde_json::json!({"output":output.stdout}),
            )
        })?;
    }
    openrust::support::facades::Route::get("/display/failure", || async {
        let mut output = Output::default();
        openrust::support::facades::Artisan::call(
            "display:show",
            &["--invalid".into()],
            &mut output,
        )
        .await
        .map_err(|source| openrust::http::HttpError::Internal {
            source: Box::new(source),
        })?;
        openrust::http::Response::json(
            http::StatusCode::OK,
            serde_json::json!({"output":output.stdout}),
        )
    })?;
    Ok(())
}
#[tokio::test]
#[ignore = "由父消费者设置真实终端宽度后运行HTTP服务"]
async fn display_http_driver() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::configuration::Routing};
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().web(routes).commands(commands))
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
    let oracle: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/display-console-oracle.json"
    ))
    .unwrap();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    for kind in ["Alert", "BulletList", "TwoColumnDetail"] {
        let expected = oracle
            .iter()
            .find(|case| {
                case["kind"] == kind
                    && case["side"] == "stdout"
                    && case["width"] == 80
                    && case["level"] == 32
                    && case["decorated"] == false
            })
            .unwrap()["stdout"]
            .clone();
        for (path, status) in [
            (format!("/display/{kind}"), 200),
            ("/display/failure".to_owned(), 500),
            (format!("/display/{kind}"), 200),
        ] {
            let response = client
                .get(format!("http://{address}{path}"))
                .header("Accept", "application/json")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            if status == 200 {
                assert_eq!(response.json::<Value>().await.unwrap()["output"], expected);
            }
        }
    }
    let mut tasks = vec![];
    for n in 0..12 {
        let kind = ["Alert", "BulletList", "TwoColumnDetail"][n % 3];
        let client = client.clone();
        let expected = oracle
            .iter()
            .find(|case| {
                case["kind"] == kind
                    && case["side"] == "stdout"
                    && case["width"] == 80
                    && case["level"] == 32
                    && case["decorated"] == false
            })
            .unwrap()["stdout"]
            .clone();
        tasks.push(tokio::spawn(async move {
            let response = client
                .get(format!("http://{address}/display/{kind}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.json::<Value>().await.unwrap()["output"], expected);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    let _ = server.await;
    println!("DISPLAY_HTTP_OK");
}
#[test]
fn actual_http_widgets_recover_and_keep_parallel_outputs_separate() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "display_http_driver", "--ignored", "--nocapture"])
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("DISPLAY_HTTP_OK"));
}
