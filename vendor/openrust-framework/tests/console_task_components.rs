use openrust::{
    console::{
        AsOutput, CommandDefinition, ConsoleError, ConsoleStyle, Output, OutputFormat,
        OutputOptions, OutputReference, OutputStyle, OutputVerbosity,
        view::{TaskResult, components::Task},
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
    let formatter = output.as_output().get_formatter();
    let mut component = Task::new(output);
    if let Some(style) = case["custom_style"].as_str() {
        formatter.set_style(
            style,
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
    let runtime = case["runtime"].as_str().unwrap().to_owned();
    component.set_run_time_formatter(Some(Box::new(move |_, _| {
        if runtime == "error" {
            Err(ConsoleError::new("runtime failed"))
        } else {
            Ok(runtime.clone())
        }
    })));
    let description = case["description"]
        .as_str()
        .unwrap()
        .replace("__BASE__", base);
    let opt = options(case["required"].as_u64().unwrap());
    if case["mode"] == "none" {
        if case["required"] == 32 {
            component.render(&description)
        } else {
            component.render_with(&description, opt)
        }
    } else {
        let task = |output: &mut OutputReference<'_>| -> Result<Value, ConsoleError> {
            if let Some(body) = case["body"].as_str().filter(|v| !v.is_empty()) {
                output.write(body, false, OutputOptions::default())?;
            }
            if case["mode"] == "throw" {
                return Err(ConsoleError::new("task failed"));
            }
            Ok(match case["mode"].as_str().unwrap() {
                "enum_backing" => TaskResult::Failure.into(),
                "enum_object" => serde_json::json!({"case":"Failure"}),
                _ => case["value"].clone(),
            })
        };
        if case["required"] == 32 {
            component.render_task(&description, task)
        } else {
            component.render_task_with(&description, task, opt)
        }
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
        "style" => {
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
    if let Err(error) = &result {
        assert_eq!(error.to_string(), case["error"].as_str().unwrap());
    }
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
fn task_width_driver() {
    let width = std::env::var("COLUMNS").unwrap().parse::<i64>().unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/task-layout-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into()).create().unwrap();
    let base = root.path().display().to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut count = 0;
    rt.block_on(async {
        for case in cases.iter().filter(|case| case["width"] == width) {
            App::set_facade_application(Some(&app), async { check_case(case, &base) }).await;
            count += 1;
        }
    });
    assert!(count > 0);
    println!("TASK_CASES_OK:{count}");
}
#[test]
fn actual_task_widths_outcomes_and_output_state_match_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/task-layout-oracle.json"
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
            .args(["--exact", "task_width_driver", "--ignored", "--nocapture"])
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
            String::from_utf8_lossy(&result.stdout).contains(&format!("TASK_CASES_OK:{expected}"))
        );
        total += expected;
    }
    assert_eq!(total, cases.len());
}
#[test]
fn default_duration_format_matches_fixed_source_boundaries_and_long_runs() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/task-runtime-oracle.json"
    ))
    .unwrap();
    let mut output = Output::default();
    let mut task = Task::new(&mut output);
    for row in rows {
        let end = match row["special"].as_str() {
            Some("nan") => f64::NAN,
            Some("inf") => f64::INFINITY,
            Some("negative_inf") => f64::NEG_INFINITY,
            _ => row["seconds"].as_f64().unwrap(),
        };
        assert_eq!(
            task.run_time_for_humans(0.0, Some(end)).unwrap(),
            row["output"],
            "{row}"
        );
    }
}
#[test]
fn task_result_callback_uses_backing_and_slow_default_timing_is_real() {
    let mut output = Output::default();
    let mut task = Task::new(&mut output);
    task.render_task("Skipped", |_| Ok(TaskResult::Skipped))
        .unwrap();
    task.render_task("Slow", |selected| {
        selected.write("work:", false, OutputOptions::default())?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        Ok(false)
    })
    .unwrap();
    drop(task);
    assert!(output.stdout.contains(" SKIPPED\n"));
    assert!(output.stdout.contains("Slow work:"));
    assert!(
        regex::Regex::new(r" [1-9][0-9]*s DONE\n")
            .unwrap()
            .is_match(&output.stdout),
        "{}",
        output.stdout
    );
}
#[test]
fn panic_and_callback_error_finish_before_propagation_and_allow_reuse() {
    let mut output = Output::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Task::new(&mut output)
            .render_task("Panic", |_| -> Result<(), ConsoleError> {
                panic!("sentinel")
            })
            .unwrap();
    }));
    assert!(result.is_err());
    assert!(output.stdout.contains(" FAIL\n"));
    let error = Task::new(&mut output)
        .render_task("Error", |_| -> Result<(), ConsoleError> {
            Err(ConsoleError::new("sentinel"))
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "sentinel");
    Task::new(&mut output).render("Recovered").unwrap();
    assert!(output.stdout.ends_with(" DONE\n"));
}
fn render_command(output: &mut (impl AsOutput + ?Sized), mode: &str) -> Result<(), ConsoleError> {
    let mut task = Task::new(output);
    task.set_run_time_formatter(Some(Box::new(|_, _| Ok("0.42ms".into()))));
    task.render_task("Build [report].", |selected| {
        selected.write("body\n", false, OutputOptions::default())?;
        match mode {
            "error" => Err(ConsoleError::new("task failed")),
            "failure" => Ok(TaskResult::Failure.into()),
            "skipped" => Ok(TaskResult::Skipped.into()),
            "false" => Ok(Value::Bool(false)),
            _ => Ok(TaskResult::Success.into()),
        }
    })
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "tasks:render {--outcome=success} {--stderr}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .option("outcome")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "success".into());
                let mut style = OutputStyle::new(input, output);
                if input.option("stderr").and_then(|v| v.as_bool()) == Some(true) {
                    render_command(&mut style.get_error_style(), &mode)?;
                } else {
                    render_command(&mut style, &mode)?;
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/tasks", task_response)?;
    Ok(())
}
fn task_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let mode = request
            .query(Some("mode"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "success".into());
        let mut output = Output::default();
        let result = openrust::support::facades::Artisan::call(
            "tasks:render",
            &[format!("--outcome={mode}").into()],
            &mut output,
        )
        .await;
        let status = match result {
            Ok(0) => http::StatusCode::OK,
            _ => http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        openrust::http::Response::json(
            status,
            serde_json::json!({"output":output.stdout,"error":output.stderr}),
        )
    })
}
#[test]
#[ignore = "由父消费者执行真实Artisan进程及物理输出"]
fn task_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let mode = std::env::var("TASK_MODE").unwrap();
    let mut args: Vec<std::ffi::OsString> = vec![
        "tasks:render".into(),
        format!("--outcome={mode}").into(),
        "--no-ansi".into(),
    ];
    if std::env::var_os("TASK_STDERR").is_some() {
        args.push("--stderr".into());
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_exit_status_and_selected_physical_stream() {
    for (mode, label, code) in [
        ("success", "DONE", 0),
        ("failure", "FAIL", 0),
        ("skipped", "SKIPPED", 0),
        ("false", "DONE", 0),
        ("error", "FAIL", 1),
    ] {
        for stderr in [false, true] {
            let mut process = std::process::Command::new(std::env::current_exe().unwrap());
            process
                .args(["--exact", "task_cli_driver", "--ignored", "--nocapture"])
                .env("COLUMNS", "80")
                .env("TASK_MODE", mode);
            if stderr {
                process.env("TASK_STDERR", "1");
            }
            let output = process.output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(code),
                "{mode}, stderr={stderr}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let selected = String::from_utf8_lossy(if stderr {
                &output.stderr
            } else {
                &output.stdout
            });
            assert!(selected.contains("  Build [report] body\n"), "{selected}");
            assert!(
                selected.contains(&format!(" 0.42ms {label}\n")),
                "{selected}"
            );
            if stderr {
                assert!(!String::from_utf8_lossy(&output.stdout).contains("Build [report]"));
            } else {
                assert!(!String::from_utf8_lossy(&output.stderr).contains("Build [report]"));
            }
        }
    }
}
#[tokio::test]
async fn actual_http_artisan_failure_recovery_and_parallel_requests() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<
            std::sync::Arc<openrust::foundation::http::Kernel>,
        >,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(routes)
                .commands(commands),
        )
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport
                .with_state(std::sync::Arc::new(kernel))
                .into_make_service(),
        )
        .await
        .unwrap()
    });
    for (mode, status, label) in [
        ("success", 200, "DONE"),
        ("error", 500, "FAIL"),
        ("failure", 200, "FAIL"),
        ("skipped", 200, "SKIPPED"),
        ("false", 200, "DONE"),
        ("success", 200, "DONE"),
    ] {
        let response = reqwest::get(format!("http://{address}/tasks?mode={mode}"))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let response: Value = response.json().await.unwrap();
        assert!(
            response["output"]
                .as_str()
                .unwrap()
                .contains(&format!(" {label}\n")),
            "{response}"
        );
    }
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..12 {
        requests.spawn(async move {
            let response = reqwest::get(format!("http://{address}/tasks"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let response: Value = response.json().await.unwrap();
            assert_eq!(
                response["output"]
                    .as_str()
                    .unwrap()
                    .matches(" DONE\n")
                    .count(),
                1
            );
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn no_callback_skips_runtime_and_formatter_restoration_uses_default() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let shared = calls.clone();
    let mut output = Output::default();
    let mut task = Task::new(&mut output);
    task.set_run_time_formatter(Some(Box::new(move |start, end| {
        assert!(start.is_finite());
        assert!(end.is_none());
        shared.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok("custom".into())
    })));
    task.render("No callback").unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    task.render_task("Callback", |_| Ok(())).unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    task.set_run_time_formatter(None);
    assert_eq!(
        task.run_time_for_humans(0.0, Some(0.1)).unwrap(),
        "100.00ms"
    );
    drop(task);
    assert!(output.stdout.lines().next().unwrap().ends_with(" DONE"));
    assert!(!output.stdout.lines().next().unwrap().contains("custom"));
    assert!(output.stdout.contains(" custom DONE\n"));
}
