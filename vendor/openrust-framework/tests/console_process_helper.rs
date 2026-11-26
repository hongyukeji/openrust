#![cfg(unix)]

use openrust::{
    console::{
        ConsoleError, DebugFormatterHelper, HelperSet, Output, OutputVerbosity, ProcessHelper,
    },
    process::{Error, Factory, OutputStream},
};
use serde_json::Value;
use std::{error::Error as _, sync::Arc, time::Duration};

fn helpers() -> (HelperSet, Arc<ProcessHelper>, Arc<DebugFormatterHelper>) {
    let helpers = HelperSet::new();
    let formatter = Arc::new(DebugFormatterHelper::new());
    let helper = Arc::new(ProcessHelper::new());
    helpers.set(formatter.clone(), None);
    helpers.set(helper.clone(), None);
    (helpers, helper, formatter)
}

fn level(value: u64) -> OutputVerbosity {
    match value {
        32 => OutputVerbosity::Normal,
        128 => OutputVerbosity::VeryVerbose,
        256 => OutputVerbosity::Debug,
        _ => panic!("未知输出详细级别 {value}"),
    }
}

#[tokio::test]
async fn real_processes_match_fixed_run_and_must_run() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/process-helper-oracle.json"
    ))
    .unwrap();
    for row in rows {
        let (_helpers, helper, _) = helpers();
        let mut output = Output::default();
        output.set_verbosity(level(row["verbosity"].as_u64().unwrap()));
        let argv = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let command = Factory::new().command(argv);
        let mut seen = Vec::new();
        let mut callback = |stream, bytes: &[u8]| {
            seen.push((
                if stream == OutputStream::Stderr {
                    "err"
                } else {
                    "out"
                },
                String::from_utf8(bytes.to_vec()).unwrap(),
            ));
        };
        let result = if row["method"] == "run" {
            helper
                .run_with(
                    &mut output,
                    command,
                    Some("Failed <info>"),
                    Some(&mut callback),
                    OutputVerbosity::VeryVerbose,
                )
                .await
        } else {
            helper
                .must_run_with(
                    &mut output,
                    command,
                    Some("Failed <info>"),
                    Some(&mut callback),
                    OutputVerbosity::VeryVerbose,
                )
                .await
        };
        let process = if row["error"].is_null() {
            result.as_ref().unwrap()
        } else {
            let error = result
                .as_ref()
                .err()
                .unwrap()
                .source()
                .unwrap()
                .downcast_ref::<Error>()
                .unwrap();
            assert!(matches!(error, Error::Failed(_)));
            error.result().unwrap()
        };
        assert_eq!(process.exit_code(), row["exit"]);
        assert_eq!(
            std::str::from_utf8(process.output()).unwrap(),
            row["stdout"]
        );
        assert_eq!(
            std::str::from_utf8(process.error_output()).unwrap(),
            row["stderr"]
        );
        assert_eq!(serde_json::to_value(seen).unwrap(), row["callback"]);
        assert_eq!(output.stdout, row["log"], "{row}");
    }
}

#[tokio::test]
async fn manual_wrapper_buffers_unmodified_bytes_and_typed_process_errors() {
    let (_helpers, helper, debug) = helpers();
    let mut output = Output::default();
    let mut seen = Vec::new();
    {
        debug.start("explicit", "ready");
        let mut mapped = |_, data: &[u8]| seen.extend_from_slice(data);
        let mut wrap = helper
            .wrap_callback(&mut output, "explicit", Some(&mut mapped))
            .unwrap();
        wrap(OutputStream::Stdout, b"untrusted<info>\xff").unwrap();
    }
    assert_eq!(seen, b"untrusted<info>\xff");
    assert!(output.stdout.contains("untrusted<info>"));
    let mut missing = helper.wrap_callback(&mut output, "missing", None).unwrap();
    assert!(missing(OutputStream::Stdout, b"x").is_err());
    drop(missing);

    let error = helper
        .run(&mut output, Vec::<String>::new())
        .await
        .err()
        .unwrap();
    assert!(matches!(
        error.source().unwrap().downcast_ref::<Error>(),
        Some(Error::InvalidCommand)
    ));
    let command = Factory::new()
        .command(["/bin/sh", "-c", "exec sleep 5"])
        .timeout(Duration::from_millis(40));
    let error = helper.run(&mut output, command).await.err().unwrap();
    assert!(matches!(
        error.source().unwrap().downcast_ref::<Error>(),
        Some(Error::TimedOut { .. })
    ));
    let value = helper
        .run(&mut output, ["/bin/echo", "literal ; <text>"])
        .await
        .unwrap();
    assert_eq!(value.output(), b"literal ; <text>\n");
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "helpers:process {status=0}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let (_set, helper, _) = helpers();
                let failed = input.argument("status") == Some(Value::String("7".into()));
                let script = if failed {
                    "printf bad >&2; exit 7"
                } else {
                    "printf 'ok<info>'"
                };
                let process = helper.must_run(output, ["/bin/sh", "-c", script]).await?;
                output.comment(std::str::from_utf8(process.output()).unwrap());
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父消费者启动真实 Artisan 进程"]
fn process_helper_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "helpers:process".into(),
        std::env::var("PROCESS_HELPER_STATUS").unwrap().into(),
        "--no-ansi".into(),
        "-vvv".into(),
    ];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let status = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(status);
}

#[test]
fn actual_artisan_diagnostics_follow_selected_error_stream() {
    for status in ["0", "7"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process_helper_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("PROCESS_HELPER_STATUS", status)
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            output.status.code(),
            Some(if status == "0" { 0 } else { 1 }),
            "{stdout}\n{stderr}"
        );
        assert!(!stdout.contains(" RUN "), "{stdout}");
        assert!(stderr.contains(" RUN "), "{stderr}");
        assert!(
            stderr.contains(if status == "0" {
                "Command ran successfully"
            } else {
                "7 Command did not run successfully"
            }),
            "{stderr}"
        );
    }
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/process-helper", response)?;
    Ok(())
}

fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let status = request
            .query(Some("status"))?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "0".into());
        let mut output = Output::default();
        let code = openrust::support::facades::Artisan::call(
            "helpers:process",
            &[status.into(), "-vvv".into(), "--no-ansi".into()],
            &mut output,
        )
        .await;
        openrust::http::Response::json(
            if matches!(code, Ok(0)) {
                http::StatusCode::OK
            } else {
                http::StatusCode::BAD_GATEWAY
            },
            serde_json::json!({"output": output.stdout}),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_dispatch_preserves_process_diagnostics_and_failure_recovery() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
    };
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let application = Application::configure(root.path().into())
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
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
    let client = reqwest::Client::new();
    let failed = client
        .get(format!("http://{address}/process-helper?status=7"))
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 502);
    let failed: Value = failed.json().await.unwrap();
    assert!(
        failed["output"]
            .as_str()
            .unwrap()
            .contains("7 Command did not run successfully")
    );
    let mut calls = tokio::task::JoinSet::new();
    for _ in 0..6 {
        let client = client.clone();
        calls.spawn(async move {
            let response = client
                .get(format!("http://{address}/process-helper?status=0"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            let output = body["output"].as_str().unwrap();
            assert!(output.contains("ok<info>"), "{output}");
            assert!(output.contains("Command ran successfully"), "{output}");
        });
    }
    while let Some(result) = calls.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
