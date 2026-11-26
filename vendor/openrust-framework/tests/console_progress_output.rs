use openrust::{
    console::{
        ConsoleError, ConsoleFormatError, Input, Output, OutputFormat, OutputOptions,
        OutputVerbosity, ProgressBar, ProgressOutput,
    },
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-output-oracle.json"
    ))
    .unwrap()
}
fn arguments(case: &Value) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        case.to_string().into(),
        if case["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    match case["level"].as_u64().unwrap() {
        8 => args.push("--silent".into()),
        16 => args.push("--quiet".into()),
        64 => args.push("-v".into()),
        256 => args.push("-vvv".into()),
        _ => {}
    };
    args
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "progress:output {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let metadata = Arc::new(Mutex::new(Vec::<Value>::new()));
            let log = metadata.clone();
            let mut io = input.io(output);
            let mut bar = if case["styled"] == true {
                io.get_output().create_progress_bar(2)?
            } else {
                ProgressBar::new(io.get_output().get_output(), 2)?
            };
            let format = match case["format"].as_u64().unwrap() {
                2 => OutputFormat::Raw,
                4 => OutputFormat::Plain,
                _ => OutputFormat::Normal,
            };
            bar.set_format("%custom%")
                .min_seconds_between_redraws(1000.0)
                .set_placeholder_formatter(
                    "custom",
                    Arc::new(move |bar| {
                        let current = bar.get_progress();
                        let mut stream: ProgressOutput<'_> = bar.get_output();
                        log.lock().unwrap().push(json!([
                            stream.get_verbosity() as u16,
                            stream.is_decorated(),
                            stream.is_quiet(),
                            stream.is_verbose(),
                            stream.is_very_verbose(),
                            stream.is_debug()
                        ]));
                        stream.write_iter(
                            ["<info>note</info>".to_owned(), format!("/{current}/")],
                            false,
                            format,
                        )?;
                        stream.writeln(["<comment>row</comment>", "tail"].as_slice(), format)?;
                        stream.write(
                            "hidden",
                            false,
                            OutputOptions {
                                format,
                                verbosity: Some(OutputVerbosity::Debug),
                            },
                        )?;
                        Ok(format!("bar:{current}"))
                    }),
                );
            bar.start()?;
            bar.advance_by(2)?;
            bar.finish()?;
            drop(bar);
            assert_eq!(json!(*metadata.lock().unwrap()), case["metadata"], "{case}");
            assert_eq!(
                json!(metadata.lock().unwrap().len()),
                case["calls"],
                "{case}"
            );
            assert_eq!(
                json!(io.get_output().new_lines_written()),
                case["new_lines"]
            );
            assert_eq!(json!(io.get_output().new_line_written()), case["new_line"]);
            drop(io);
            if case["stream"] != true {
                assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
            }
            Ok(0)
        },
    )?;
    Artisan::command_with(
        "progress:output-error",
        |input: &Input, output: &mut Output| {
            let mut io = input.io(output);
            let mut bar = io.get_output().create_progress_bar(1)?;
            bar.get_output()
                .writeln("<fg=not-a-color>bad</>", OutputOptions::default())?;
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn output_view_callbacks_match_real_source_formats_metadata_and_verbosity() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases() {
            Artisan::call("progress:output", &arguments(&case), &mut Output::default())
                .await
                .unwrap();
        }
    })
    .await;
}
#[test]
#[ignore = "由实际Artisan子进程驱动"]
fn output_cli_fixture() {
    let case: Value =
        serde_json::from_str(&std::env::var("PROGRESS_OUTPUT_CASE").unwrap()).unwrap();
    let mut args = arguments(&case);
    args.insert(0, "progress:output".into());
    let app = Application::configure(std::env::var("PROGRESS_OUTPUT_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    std::process::exit(code);
}
#[test]
fn actual_artisan_callback_output_targets_stderr_without_changing_stdout() {
    let root = tempfile::tempdir().unwrap();
    let streams: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-output-cli-oracle.json"
    ))
    .unwrap();
    for (index, mut case) in cases().into_iter().enumerate() {
        case["stream"] = json!(true);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "output_cli_fixture", "--ignored", "--nocapture"])
            .env("PROGRESS_OUTPUT_ROOT", root.path())
            .env("PROGRESS_OUTPUT_CASE", case.to_string())
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(
            String::from_utf8(child.stderr).unwrap(),
            streams[index]["stderr"].as_str().unwrap(),
            "{case}"
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(
            stdout.strip_prefix("\nrunning 1 test\n").unwrap(),
            streams[index]["stdout"].as_str().unwrap(),
            "{case}"
        );
    }
}
#[test]
fn direct_stream_operations_keep_the_concrete_formatter_error_and_recover() {
    let mut output = Output::default();
    let mut bar = ProgressBar::new(&mut output, 1).unwrap();
    let error = bar
        .get_output()
        .write("<fg=not-a-color>bad</>", false, OutputOptions::default())
        .unwrap_err();
    assert!(matches!(
        std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<ConsoleFormatError>(),
        Some(ConsoleFormatError::InvalidColor(_))
    ));
    bar.get_output()
        .writeln_iter(["one", "two"], OutputFormat::Raw)
        .unwrap();
    bar.get_output()
        .write(["three", "four"].as_slice(), false, OutputFormat::Plain)
        .unwrap();
    drop(bar);
    assert_eq!(output.stdout, "one\ntwo\nthreefour");
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    for (path, index) in [
        ("/output", Some(12)),
        ("/output/plain", Some(16)),
        ("/output/error", None),
    ] {
        Route::get(path, move || async move {
            let mut output = Output::default();
            if let Some(index) = index {
                Artisan::call("progress:output", &arguments(&cases()[index]), &mut output).await
            } else {
                Artisan::call("progress:output-error", &[], &mut output).await
            }
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_output_errors_and_recovery_use_the_public_entry() {
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
    for (path, status) in [
        ("/output", 200),
        ("/output/plain", 200),
        ("/output/error", 500),
        ("/output", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert!(
                response.json::<Value>().await.unwrap()["output"]
                    .as_str()
                    .unwrap()
                    .contains("bar:2")
            );
        }
    }
    server.abort();
    let _ = server.await;
}
