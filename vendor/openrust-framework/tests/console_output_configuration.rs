use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputVerbosity},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::{Value, json};
fn level(value: u64) -> OutputVerbosity {
    match value {
        8 => OutputVerbosity::Silent,
        16 => OutputVerbosity::Quiet,
        64 => OutputVerbosity::Verbose,
        128 => OutputVerbosity::VeryVerbose,
        256 => OutputVerbosity::Debug,
        _ => OutputVerbosity::Normal,
    }
}
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/output-configuration-oracle.json"
    ))
    .unwrap()
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "output:configure {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut io = input.io(output);
            let mut trace = Vec::new();
            for op in case["ops"].as_array().unwrap() {
                let style = io.get_output();
                match op[0].as_str().unwrap() {
                    "write" => {
                        style.write(op[1].as_str().unwrap(), false, OutputOptions::default())?
                    }
                    "quiet" => {
                        style.write(op[1].as_str().unwrap(), false, OutputVerbosity::Quiet)?
                    }
                    "verbose" => {
                        style.write(op[1].as_str().unwrap(), false, OutputVerbosity::Verbose)?
                    }
                    "tag" => style.write(
                        format!("<info>{}</info>", op[1].as_str().unwrap()).as_str(),
                        false,
                        OutputOptions::default(),
                    )?,
                    "setVerbosity" => style.set_verbosity(level(op[1].as_u64().unwrap())),
                    "setDecorated" => style.set_decorated(op[1].as_bool().unwrap()),
                    _ => panic!("未知配置操作"),
                }
                trace.push(json!([
                    style.get_verbosity() as u16,
                    style.is_decorated(),
                    style.is_silent(),
                    style.is_quiet(),
                    style.is_verbose(),
                    style.is_very_verbose(),
                    style.is_debug(),
                    style.new_lines_written(),
                    style.new_line_written()
                ]));
            }
            assert_eq!(json!(trace), case["trace"], "{case}");
            Ok(0)
        },
    )?;
    register_streams()?;
    register_nested()?;
    Ok(())
}
#[tokio::test]
async fn source_output_levels_decoration_and_already_emitted_content_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases() {
            let mut args = vec![case.to_string().into(), "--no-ansi".into()];
            match case["initial"].as_u64().unwrap() {
                8 => args.push("--silent".into()),
                16 => args.push("--quiet".into()),
                _ => {}
            }
            let mut output = Output::default();
            Artisan::call("output:configure", &args, &mut output)
                .await
                .unwrap();
            assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
        }
    })
    .await;
}
fn register_streams() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "output:streams {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let terminal = output.streams_to_terminal();
            let target = level(case["level"].as_u64().unwrap());
            let decorated = case["decorated"].as_bool().unwrap();
            let mut io = input.io(output);
            io.get_output()
                .write("before;", false, OutputOptions::default())?;
            let mut bar = io.get_output().create_progress_bar(1)?;
            bar.set_format("p%current%")
                .min_seconds_between_redraws(1000.0);
            {
                let mut stream = bar.get_output();
                stream.set_verbosity(target);
                stream.set_decorated(decorated);
                assert_eq!(stream.get_verbosity(), target);
                assert_eq!(stream.is_decorated(), decorated);
                stream.write(
                    "<info>error-before;</info>",
                    false,
                    OutputOptions::default(),
                )?;
            }
            bar.start()?;
            bar.advance()?;
            bar.finish()?;
            drop(bar);
            let style = io.get_output();
            assert_eq!(
                style.get_verbosity(),
                if terminal {
                    OutputVerbosity::Normal
                } else {
                    target
                }
            );
            assert_eq!(style.is_decorated(), !terminal && decorated);
            style.write(
                format!(
                    "main:{}:{};",
                    style.get_verbosity() as u16,
                    u8::from(style.is_decorated())
                )
                .as_str(),
                false,
                OutputOptions::default(),
            )?;
            style.set_verbosity(OutputVerbosity::Quiet);
            style.write("hidden", false, OutputOptions::default())?;
            style.write("quiet;", false, OutputVerbosity::Quiet)?;
            style.set_verbosity(OutputVerbosity::Normal);
            style.set_decorated(false);
            let mut bar = style.create_progress_bar(1)?;
            bar.set_format("q%current%")
                .min_seconds_between_redraws(1000.0);
            bar.start()?;
            bar.advance()?;
            bar.finish()?;
            drop(bar);
            io.get_output()
                .write("after;", false, OutputOptions::default())?;
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn buffered_progress_configuration_uses_the_same_stream_and_primary_setter_resets_it() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let expected:Vec<Value>=serde_json::from_str(include_str!("../../../.agents/project/audits/artisan-tutorial/output-stream-configuration-buffer-oracle.json")).unwrap();
    App::set_facade_application(Some(&app), async {
        for (index, (level, decorated)) in [8, 16, 32, 64, 128, 256]
            .into_iter()
            .flat_map(|level| [false, true].map(move |decorated| (level, decorated)))
            .enumerate()
        {
            let mut output = Output::default();
            Artisan::call(
                "output:streams",
                &[
                    json!({"level":level,"decorated":decorated})
                        .to_string()
                        .into(),
                    "--no-ansi".into(),
                ],
                &mut output,
            )
            .await
            .unwrap();
            assert_eq!(output.stdout, expected[index]["stdout"].as_str().unwrap());
            assert_eq!(output.stderr, expected[index]["stderr"].as_str().unwrap());
        }
    })
    .await;
}
#[test]
#[ignore = "由实际Artisan子进程驱动"]
fn configuration_cli_fixture() {
    let args: Vec<std::ffi::OsString> =
        serde_json::from_str::<Vec<String>>(&std::env::var("CONFIGURATION_ARGS").unwrap())
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
    let app = Application::configure(std::env::var("CONFIGURATION_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    std::process::exit(code);
}
#[test]
fn actual_artisan_configures_stderr_independently_and_resets_both_from_the_primary_style() {
    let root = tempfile::tempdir().unwrap();
    let expected:Vec<Value>=serde_json::from_str(include_str!("../../../.agents/project/audits/artisan-tutorial/output-stream-configuration-cli-oracle.json")).unwrap();
    for (index, (level, decorated)) in [8, 16, 32, 64, 128, 256]
        .into_iter()
        .flat_map(|level| [false, true].map(move |decorated| (level, decorated)))
        .enumerate()
    {
        let args = json!([
            "output:streams",
            json!({"level":level,"decorated":decorated}).to_string(),
            "--no-ansi"
        ]);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "configuration_cli_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("CONFIGURATION_ROOT", root.path())
            .env("CONFIGURATION_ARGS", args.to_string())
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
            expected[index]["stderr"].as_str().unwrap(),
            "level={level},decorated={decorated}"
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(
            stdout.strip_prefix("\nrunning 1 test\n").unwrap(),
            expected[index]["stdout"].as_str().unwrap()
        );
    }
}
fn register_nested() -> Result<(), ConsoleError> {
    use openrust::console::AsyncCommandCallback;
    use std::sync::Arc;
    let entered = Arc::new(tokio::sync::Notify::new());
    let wait_entered = entered.clone();
    Artisan::command_with("output:child", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut bar = io.get_output().create_progress_bar(1)?;
        assert_eq!(bar.get_output().get_verbosity(), OutputVerbosity::Quiet);
        assert!(!bar.get_output().is_decorated());
        bar.set_format("child");
        bar.start()?;
        bar.advance()?;
        bar.finish()?;
        Ok(0)
    })?;
    Artisan::command_with(
        "output:wait",
        AsyncCommandCallback::new(move |_, input, output| {
            let entered = wait_entered.clone();
            Box::pin(async move {
                let mut io = input.io(output);
                let mut bar = io.get_output().create_progress_bar(1)?;
                bar.get_output().set_verbosity(OutputVerbosity::Silent);
                bar.get_output().set_decorated(false);
                entered.notify_one();
                std::future::pending::<()>().await;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "output:parent",
        AsyncCommandCallback::new(move |_, input, output| {
            let entered = entered.clone();
            Box::pin(async move {
                {
                    let mut io = input.io(output);
                    let mut bar = io.get_output().create_progress_bar(1)?;
                    bar.get_output().set_verbosity(OutputVerbosity::Debug);
                    bar.get_output().set_decorated(true);
                }
                Artisan::call(
                    "output:child",
                    &["--quiet".into(), "--no-ansi".into()],
                    output,
                )
                .await?;
                {
                    let future = Artisan::call("output:wait", &[], output);
                    let mut future = Box::pin(future);
                    tokio::select! {_=&mut future=>panic!("等待命令不应自行结束"),_=entered.notified()=>{}}
                }
                assert_eq!(output.verbosity(), OutputVerbosity::Normal);
                assert!(!output.is_decorated());
                {
                    let mut io = input.io(output);
                    let mut bar = io.get_output().create_progress_bar(1)?;
                    assert_eq!(bar.get_output().get_verbosity(), OutputVerbosity::Debug);
                    assert!(bar.get_output().is_decorated());
                }
                input
                    .io(output)
                    .get_output()
                    .write("restored", false, OutputOptions::default())?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
fn nested_cli_flags_and_cancellation_restore_the_parent_output_stream() {
    let root = tempfile::tempdir().unwrap();
    let args = json!(["output:parent", "--no-ansi"]);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "configuration_cli_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("CONFIGURATION_ROOT", root.path())
        .env("CONFIGURATION_ARGS", args.to_string())
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(String::from_utf8(child.stderr).unwrap(), "");
    assert_eq!(
        String::from_utf8(child.stdout)
            .unwrap()
            .strip_prefix("\nrunning 1 test\n")
            .unwrap(),
        "restored"
    );
}
#[test]
fn plain_output_setters_are_public_and_the_style_reads_the_same_backend() {
    use openrust::console::{CommandDefinition, OutputStyle};
    let input = CommandDefinition::from_signature("configuration")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    output.set_verbosity(OutputVerbosity::VeryVerbose);
    output.set_decorated(true);
    assert!(output.is_very_verbose());
    assert!(!output.is_debug());
    {
        let mut style = OutputStyle::new(&input, &mut output);
        assert_eq!(style.get_verbosity(), OutputVerbosity::VeryVerbose);
        assert!(style.is_decorated());
        style
            .write("<info>visible</info>", false, OutputVerbosity::VeryVerbose)
            .unwrap();
        style.set_verbosity(OutputVerbosity::Normal);
        style.set_decorated(false);
        style
            .write("<info>plain</info>", false, OutputOptions::default())
            .unwrap();
    }
    assert_eq!(output.verbosity(), OutputVerbosity::Normal);
    assert!(!output.is_decorated());
    assert_eq!(output.stdout, "\x1b[32mvisible\x1b[39mplain");
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/configuration/{index}", configuration_response)?;
    Ok(())
}
fn configuration_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let index = request.route("index").unwrap().parse::<usize>().unwrap();
        let mut output = Output::default();
        let result = if index == 999 {
            Artisan::call("output:not-registered", &[], &mut output).await
        } else {
            let case = &cases()[index];
            let mut args = vec![case.to_string().into(), "--no-ansi".into()];
            match case["initial"].as_u64().unwrap() {
                8 => args.push("--silent".into()),
                16 => args.push("--quiet".into()),
                _ => {}
            };
            Artisan::call("output:configure", &args, &mut output).await
        };
        result.map_err(|source| openrust::http::HttpError::Internal {
            source: Box::new(source),
        })?;
        openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
    })
}
#[tokio::test]
async fn actual_http_runtime_configuration_and_errors_preserve_later_requests() {
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
    for index in [0, 1, 2, 12, 13, 14, 999, 0] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}/configuration/{index}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), if index == 999 { 500 } else { 200 });
        if index != 999 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["output"],
                cases()[index]["output"]
            );
        }
    }
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn clearing_and_reusing_a_captured_output_does_not_duplicate_old_visibility_ranges() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let mut output = Output::default();
        let first = &cases()[14];
        Artisan::call("output:configure", &[first.to_string().into()], &mut output)
            .await
            .unwrap();
        assert_eq!(output.stdout, "restored");
        output.stdout.clear();
        let second = &cases()[2];
        Artisan::call(
            "output:configure",
            &[second.to_string().into(), "--silent".into()],
            &mut output,
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, "restored");
    })
    .await;
}
