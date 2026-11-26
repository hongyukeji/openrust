use openrust::{
    console::{
        AsyncCommandCallback, CommandDefinition, ConsoleError, ConsoleFormatError, Input, Output,
        OutputFormat, OutputOptions, OutputVerbosity,
    },
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};

fn commands() -> Result<(), ConsoleError> {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    Artisan::command_with("io:style {level}", |input: &Input, output: &mut Output| {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../.agents/project/audits/artisan-tutorial/output-style-oracle.json"
        ))
        .unwrap();
        let level = input
            .argument("level")
            .unwrap()
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let mut io = input.io(output);
        let style = io.get_output();
        assert_eq!(style.new_lines_written(), 1);
        assert!(!style.new_line_written());
        for case in cases
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["level"] == level)
        {
            let before = style.get_output().stdout.len();
            let numeric = case["options"].as_u64().unwrap();
            let options = match numeric {
                2 => OutputFormat::Raw.into(),
                4 => OutputFormat::Plain.into(),
                8 => OutputVerbosity::Silent.into(),
                16 => OutputVerbosity::Quiet.into(),
                18 => OutputOptions {
                    format: OutputFormat::Raw,
                    verbosity: Some(OutputVerbosity::Quiet),
                },
                20 => OutputOptions {
                    format: OutputFormat::Plain,
                    verbosity: Some(OutputVerbosity::Quiet),
                },
                64 => OutputVerbosity::Verbose.into(),
                128 => OutputVerbosity::VeryVerbose.into(),
                256 => OutputVerbosity::Debug.into(),
                _ => OutputOptions::default(),
            };
            if case["method"] == "newLine" {
                style.new_line(case["message"].as_u64().unwrap() as usize)?;
            } else {
                let strings: Vec<_> = case["message"]
                    .as_array()
                    .map(|values| values.iter().map(|value| value.as_str().unwrap()).collect())
                    .unwrap_or_default();
                let messages = case["message"]
                    .as_str()
                    .map(openrust::console::OutputMessages::Text)
                    .unwrap_or(openrust::console::OutputMessages::Lines(&strings));
                if case["message"].is_array() {
                    if case["method"] == "write" {
                        style.write_iter(
                            strings.iter().map(|value| value.to_string()),
                            case["newline"].as_bool().unwrap(),
                            options,
                        )?;
                    } else {
                        style
                            .writeln_iter(strings.iter().map(|value| value.to_string()), options)?;
                    }
                } else if case["method"] == "write" {
                    style.write(messages, case["newline"].as_bool().unwrap(), options)?;
                } else {
                    style.writeln(messages, options)?;
                }
            }
            assert_eq!(
                &style.get_output().stdout[before..],
                case["output"].as_str().unwrap(),
                "{case}"
            );
            assert_eq!(
                style.new_lines_written() as u64,
                case["new_lines"].as_u64().unwrap(),
                "{case}"
            );
            assert_eq!(
                style.new_line_written(),
                case["new_line"].as_bool().unwrap(),
                "{case}"
            );
            assert_eq!(
                json!([
                    style.is_quiet(),
                    style.is_verbose(),
                    style.is_very_verbose(),
                    style.is_debug()
                ]),
                case["flags"],
                "{case}"
            );
        }
        Ok(0)
    })?;
    Artisan::command_with("io:threshold", |input: &Input, output: &mut Output| {
        output.info("HIDDEN_COMPONENT");
        let mut io = input.io(output);
        io.info("NORMAL_MESSAGE")?;
        io.line_with("QUIET_LINE", Some("info"), Some(OutputVerbosity::Quiet))?;
        io.info_with("QUIET_INFO", OutputVerbosity::Quiet)?;
        io.comment_with("QUIET_COMMENT", OutputVerbosity::Quiet)?;
        io.question_with("QUIET_QUESTION", OutputVerbosity::Quiet)?;
        io.warn_with("QUIET_WARN", OutputVerbosity::Quiet)?;
        io.error_with("QUIET_ERROR", OutputVerbosity::Quiet)?;
        io.alert_with("QUIET_ALERT", OutputVerbosity::Quiet)?;
        io.line_with("VERBOSE_LINE", None, Some(OutputVerbosity::Verbose))?;
        io.line_with(
            "VERY_VERBOSE_LINE",
            None,
            Some(OutputVerbosity::VeryVerbose),
        )?;
        io.line_with("DEBUG_LINE", None, Some(OutputVerbosity::Debug))?;
        Ok(0)
    })?;
    Artisan::command_with(
        "io:threshold-parent",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                input.io(output).line("PARENT_BEFORE")?;
                Artisan::call("io:threshold", &["--quiet".into()], output).await?;
                input.io(output).line("PARENT_AFTER")?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "io:captured",
        AsyncCommandCallback::new(move |_, input, output| {
            let calls = calls.clone();
            Box::pin(async move {
                tokio::task::yield_now().await;
                let count = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                input.io(output).line(&count.to_string())?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with("io:missing-service", missing_service)?;

    Artisan::command_with(
        "io:output {method} {message?} {--count=1}",
        |input: &Input, output: &mut Output| {
            let method = input.argument("method").unwrap();
            let message = input
                .argument("message")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            let mut io = input.io(output);
            assert!(io.has_argument("message"));
            assert!(io.has_argument("command"));
            assert!(!io.has_argument("unknown"));
            assert!(io.has_option("count"));
            assert!(!io.has_option("no-ansi"));
            assert!(!io.has_option("unknown"));
            match method.as_str().unwrap() {
                "line" => io.line(&message)?,
                "info" => io.info(&message)?,
                "comment" => io.comment(&message)?,
                "question" => io.question(&message)?,
                "warn" => io.warn(&message)?,
                "error" => io.error(&message)?,
                "alert" => io.alert(&message)?,
                "newLine" => {
                    io.new_line(usize::try_from(io.input().integer("count")).unwrap())?;
                }
                _ => return Err(ConsoleError::new("未知输出测试方法")),
            }
            Ok(0)
        },
    )?;
    Artisan::command_with(
        "io:parent",
        AsyncCommandCallback::new(|_: &Application, input: &Input, output: &mut Output| {
            Box::pin(async move {
                output.info("OUTER_COMPONENT");
                Artisan::call("io:output", &["info".into(), "INNER_LINE".into()], output).await?;
                input.io(output).components().info("AFTER_COMPONENT")?;
                input.io(output).line("FINAL_LINE")?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "io:pause",
        AsyncCommandCallback::new(|_: &Application, input: &Input, output: &mut Output| {
            Box::pin(async move {
                let mut io = input.io(output);
                io.info("IO_BEFORE")?;
                assert!(io.components().get_output().stdout.is_empty());
                let file =
                    std::env::var("COMMAND_IO_RELEASE").map_err(ConsoleError::from_source)?;
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
                while !Path::new(&file).exists() {
                    if tokio::time::Instant::now() > deadline {
                        return Err(ConsoleError::new("实时输出等待释放超时"));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                io.line("IO_AFTER")?;
                assert!(io.components().get_output().stdout.is_empty());
                Ok(0)
            })
        }),
    )?
    .purpose("Probe live output")
}
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
    ]))
}
fn app(path: &Path) -> Application {
    Application::configure(path.into())
        .with_config(config)
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap()
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/io-quiet", || async {
        let mut output = Output::default();
        Artisan::call("io:threshold", &["--quiet".into()], &mut output)
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        Ok::<_, openrust::http::HttpError>(output.stdout)
    })?;
    Route::get("/io-error", || async {
        let mut output = Output::default();
        Artisan::call(
            "io:output",
            &["line", "<fg=invalid>x</>"].map(Into::into),
            &mut output,
        )
        .await
        .map_err(|source| openrust::http::HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok::<_, openrust::http::HttpError>(output.stdout)
    })?;

    Route::get("/io", || async {
        let mut output = Output::default();
        Artisan::call("io:output", &["info", "Ready"].map(Into::into), &mut output)
            .await
            .unwrap();
        output.stdout
    })
}
#[tokio::test]
async fn default_output_methods_and_inline_styles_match_fixed_php_vectors() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/io-output-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        for case in cases.as_array().unwrap() {
            let mut args = vec![case["method"].as_str().unwrap().into()];
            if let Some(message) = case["message"].as_str() {
                args.push(message.into());
            } else {
                args.push(format!("--count={}", case["message"]).into());
            }
            if case["ansi"] == true {
                args.push("--ansi".into());
            }
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("io:output", &args, &mut output)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
            assert!(output.stderr.is_empty());
        }
    })
    .await;
}
#[test]
fn input_metadata_helpers_distinguish_declared_missing_values_from_unknown_names() {
    let definition = CommandDefinition::from_signature("meta {user?} {--queue=}").unwrap();
    let input = definition.parse(&[]).unwrap();
    assert!(input.has_argument("user"));
    assert!(input.has_option("queue"));
    assert!(!input.has_argument("command"));
    assert!(!input.has_option("other"));
    let mut output = Output::default();
    let io = input.io(&mut output);
    assert_eq!(io.argument("user"), None);
    assert_eq!(io.option("queue"), None);
    assert_eq!(io.input_value("unknown", json!("fallback")), "fallback");
    assert_eq!(io.arguments(), input.arguments());
    assert_eq!(io.options(), input.options());
}
#[tokio::test]
async fn formatter_errors_keep_their_types_and_subsequent_calls_can_recover() {
    use std::error::Error as _;
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        for text in [
            "<info>x</error>",
            "<fg=not-a-color>x</>",
            "<fg=#zzzzz>x</>",
            "<options=not-an-option>x</>",
        ] {
            let error = Artisan::call(
                "io:output",
                &["line".into(), text.into()],
                &mut Output::default(),
            )
            .await
            .unwrap_err();
            assert!(error.source().unwrap().is::<ConsoleFormatError>());
        }
        let mut converted = Output::default();
        Artisan::call(
            "io:output",
            &["line", "<fg=#zzzzzz>x</>"].map(Into::into),
            &mut converted,
        )
        .await
        .unwrap();
        assert_eq!(converted.stdout, "x\n");
        let mut output = Output::default();
        Artisan::call(
            "io:output",
            &["info", "Recovered"].map(Into::into),
            &mut output,
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, "Recovered\n");
    })
    .await;
}
#[tokio::test]
async fn actual_http_captures_output_without_writing_the_process_terminal() {
    use axum::handler::Handler as _;
    let root = tempfile::tempdir().unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app(root.path()));
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
        .unwrap()
    });
    let response = reqwest::get(format!("http://{address}/io")).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "Ready\n");
    let failed = reqwest::Client::new()
        .get(format!("http://{address}/io-error"))
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    assert!(
        failed.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("application/json")
    );
    assert_eq!(
        reqwest::get(format!("http://{address}/io"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "Ready\n"
    );
    let response = reqwest::get(format!("http://{address}/io-quiet"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(
        text.contains("QUIET_INFO")
            && !text.contains("NORMAL_MESSAGE")
            && !text.contains("HIDDEN_COMPONENT")
    );
    server.abort();
    let _ = server.await;
}
#[tokio::test]
#[ignore = "由真实二进制输出验收调用"]
async fn stream_fixture() {
    let root = std::env::var("COMMAND_IO_ROOT").unwrap();
    let command = std::env::var("COMMAND_IO_COMMAND").unwrap_or_else(|_| "io:pause".into());
    let mut arguments: Vec<_> = command.split_whitespace().map(Into::into).collect();
    arguments.push("--no-ansi".into());
    assert_eq!(
        app(Path::new(&root))
            .handle_command(&arguments)
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn live_binary_emits_before_completion_and_nested_components_keep_order_without_duplication()
{
    use std::io::{BufRead as _, Read as _};
    let root = tempfile::tempdir().unwrap();
    let release = root.path().join("release");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stream_fixture", "--ignored", "--nocapture"])
        .env("COMMAND_IO_ROOT", root.path())
        .env("COMMAND_IO_RELEASE", &release)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = tokio::task::spawn_blocking(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut first = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            first.push_str(&line);
            if first.contains("IO_BEFORE") {
                break;
            }
        }
        (reader, first)
    });
    let (mut reader, first) =
        match tokio::time::timeout(std::time::Duration::from_secs(3), reader).await {
            Ok(value) => value.unwrap(),
            Err(error) => {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("实时首行未到达：{error}")
            }
        };
    assert!(first.contains("IO_BEFORE"));
    assert!(child.try_wait().unwrap().is_none());
    std::fs::write(release, "release").unwrap();
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
    assert_eq!((first + &rest).matches("IO_BEFORE").count(), 1);
    assert_eq!(rest.matches("IO_AFTER").count(), 1);
    for (args, visible) in [
        ("io:parent", true),
        ("io:parent --quiet", false),
        ("io:parent --silent", false),
    ] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "stream_fixture", "--ignored", "--nocapture"])
            .env("COMMAND_IO_ROOT", root.path())
            .env("COMMAND_IO_COMMAND", args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        if visible {
            let mut previous = 0;
            for key in [
                "OUTER_COMPONENT",
                "INNER_LINE",
                "AFTER_COMPONENT",
                "FINAL_LINE",
            ] {
                assert_eq!(text.matches(key).count(), 1);
                let index = text.find(key).unwrap();
                assert!(index > previous);
                previous = index;
            }
        } else {
            assert!(
                !text.contains("_COMPONENT")
                    && !text.contains("INNER_LINE")
                    && !text.contains("FINAL_LINE")
            );
        }
    }
}

struct MissingService;
fn missing_service<'a>(
    _: Arc<MissingService>,
    _: &'a Input,
    _: &'a mut Output,
) -> openrust::console::ConsoleFuture<'a, Result<i32, ConsoleError>> {
    Box::pin(async { Ok(0) })
}
#[tokio::test]
async fn captured_async_callbacks_use_the_same_facade_and_dependency_errors_keep_their_source() {
    use std::error::Error as _;
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        for expected in ["1\n", "2\n"] {
            let mut output = Output::default();
            Artisan::call("io:captured", &[], &mut output)
                .await
                .unwrap();
            assert_eq!(output.stdout, expected);
        }
        let error = Artisan::call("io:missing-service", &[], &mut Output::default())
            .await
            .unwrap_err();
        assert!(
            error
                .source()
                .unwrap()
                .is::<openrust::contracts::container::BindingResolutionError>()
        );
    })
    .await;
}
#[tokio::test]
async fn formatting_state_is_shared_across_calls_on_the_same_output_and_not_between_outputs() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        let mut output = Output::default();
        for text in ["<info>", "green", "</info>"] {
            Artisan::call(
                "io:output",
                &["line".into(), text.into(), "--ansi".into()],
                &mut output,
            )
            .await
            .unwrap();
        }
        assert_eq!(output.stdout, "\n\x1b[32mgreen\x1b[39m\n\n");
        let mut other = Output::default();
        Artisan::call(
            "io:output",
            &["line", "plain", "--ansi"].map(Into::into),
            &mut other,
        )
        .await
        .unwrap();
        assert_eq!(other.stdout, "plain\n");
    })
    .await;
}

#[tokio::test]
async fn color_depths_hex_colors_and_hyperlinks_match_the_fixed_formatter_in_independent_processes()
{
    if let Ok(mode) = std::env::var("COMMAND_IO_COLOR_MODE") {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../.agents/project/audits/artisan-tutorial/io-color-oracle.json"
        ))
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut application = app(root.path());
        application.boot().await.unwrap();
        App::set_facade_application(Some(&application), async {
            for case in cases[&mode].as_array().unwrap() {
                let mut output = Output::default();
                Artisan::call(
                    "io:output",
                    &[
                        "line".into(),
                        case["message"].as_str().unwrap().into(),
                        "--ansi".into(),
                    ],
                    &mut output,
                )
                .await
                .unwrap();
                assert_eq!(
                    output.stdout,
                    format!("{}\n", case["output"].as_str().unwrap()),
                    "mode={mode}, {case}"
                );
            }
        })
        .await;
        return;
    }
    for mode in ["truecolor", "256color", "dumb"] {
        let result=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","color_depths_hex_colors_and_hyperlinks_match_the_fixed_formatter_in_independent_processes","--nocapture"]).env("COMMAND_IO_COLOR_MODE",mode).env("COLORTERM",mode).env("TERM","dumb").env_remove("TERMINAL_EMULATOR").env_remove("IDEA_INITIAL_DIRECTORY").env_remove("KONSOLE_VERSION").output().unwrap();
        assert!(
            result.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[tokio::test]
async fn quiet_and_silent_skip_formatting_without_changing_shared_style_state() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        for flag in ["--quiet", "--silent"] {
            let mut output = Output::default();
            for message in ["<info>", "<fg=invalid>x</>"] {
                assert_eq!(
                    Artisan::call(
                        "io:output",
                        &["line".into(), message.into(), flag.into()],
                        &mut output
                    )
                    .await
                    .unwrap(),
                    0
                );
                assert!(output.stdout.is_empty());
                assert!(output.stderr.is_empty());
            }
            Artisan::call(
                "io:output",
                &["line", "plain", "--ansi"].map(Into::into),
                &mut output,
            )
            .await
            .unwrap();
            assert_eq!(output.stdout, "plain\n");
        }
    })
    .await;
}

#[tokio::test]
async fn output_style_tracks_real_source_newlines_formats_and_verbosity() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        for (level, flag) in [
            (8, "--silent"),
            (16, "--quiet"),
            (32, "--no-ansi"),
            (64, "-v"),
            (128, "-vv"),
            (256, "-vvv"),
        ] {
            let mut output = Output::default();
            Artisan::call(
                "io:style",
                &[level.to_string().into(), flag.into()],
                &mut output,
            )
            .await
            .unwrap();
        }
    })
    .await;
}
#[test]
fn command_io_can_replace_its_input_and_its_real_output_style() {
    use openrust::console::OutputStyle;
    let definition = CommandDefinition::from_signature("read {name}").unwrap();
    let input = definition.parse(&["first".into()]).unwrap();
    let second = definition.parse(&["second".into()]).unwrap();
    let mut first_output = Output::default();
    let mut second_output = Output::default();
    {
        let mut io = input.io(&mut first_output);
        io.line("first").unwrap();
        io.set_input(&second);
        assert_eq!(io.argument("name"), Some(json!("second")));
        io.set_output(OutputStyle::new(&second, &mut second_output));
        assert_eq!(io.get_output().new_lines_written(), 1);
        io.line("second").unwrap();
        assert_eq!(io.get_output().new_lines_written(), 1);
        io.get_output().get_output().write_raw("raw").unwrap();
    }
    assert_eq!(first_output.stdout, "first\n");
    assert_eq!(second_output.stdout, "second\nraw");
    for (name, expected) in [
        ("v", OutputVerbosity::Verbose),
        ("vv", OutputVerbosity::VeryVerbose),
        ("vvv", OutputVerbosity::Debug),
        ("quiet", OutputVerbosity::Quiet),
        ("normal", OutputVerbosity::Normal),
        ("unknown", OutputVerbosity::Normal),
    ] {
        assert_eq!(OutputVerbosity::from_name(name), expected);
    }
}
#[tokio::test]
async fn explicit_quiet_messages_survive_capture_and_real_cli_but_silent_never_writes_them() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        for flag in ["--quiet", "--silent", "--no-ansi", "-v", "-vv", "-vvv"] {
            let mut output = Output::default();
            output.stdout.push_str("PREFIX\n");
            Artisan::call("io:threshold-parent", &[flag.into()], &mut output)
                .await
                .unwrap();
            if flag == "--silent" {
                assert_eq!(output.stdout, "PREFIX\n");
            } else {
                for label in [
                    "QUIET_LINE",
                    "QUIET_INFO",
                    "QUIET_COMMENT",
                    "QUIET_QUESTION",
                    "QUIET_WARN",
                    "QUIET_ERROR",
                    "QUIET_ALERT",
                ] {
                    assert_eq!(
                        output.stdout.matches(label).count(),
                        1,
                        "{flag}: {}",
                        output.stdout
                    );
                }
                assert!(!output.stdout.contains("NORMAL_MESSAGE"));
                assert!(!output.stdout.contains("HIDDEN_COMPONENT"));
                assert_eq!(output.stdout.contains("PARENT_BEFORE"), flag != "--quiet");
                assert_eq!(output.stdout.contains("PARENT_AFTER"), flag != "--quiet");
            }
        }
        let mut output = Output::default();
        for expected in 1..=3 {
            Artisan::call("io:threshold", &["--quiet".into()], &mut output)
                .await
                .unwrap();
            assert_eq!(output.stdout.matches("QUIET_INFO").count(), expected);
        }
        Artisan::call(
            "io:output",
            &["line".into(), "RESTORED".into()],
            &mut output,
        )
        .await
        .unwrap();
        assert!(output.stdout.ends_with("RESTORED\n"));
        for (flag, verbose, very, debug) in [
            ("--no-ansi", false, false, false),
            ("-v", true, false, false),
            ("-vv", true, true, false),
            ("-vvv", true, true, true),
        ] {
            let mut output = Output::default();
            Artisan::call("io:threshold", &[flag.into()], &mut output)
                .await
                .unwrap();
            assert_eq!(
                output.stdout.lines().any(|line| line == "VERBOSE_LINE"),
                verbose
            );
            assert_eq!(
                output
                    .stdout
                    .lines()
                    .any(|line| line == "VERY_VERBOSE_LINE"),
                very
            );
            assert_eq!(
                output.stdout.lines().any(|line| line == "DEBUG_LINE"),
                debug
            );
        }
    })
    .await;
    for flag in ["--quiet", "--silent", "-vvv", "-qv"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "stream_fixture", "--ignored", "--nocapture"])
            .env("COMMAND_IO_ROOT", root.path())
            .env("COMMAND_IO_COMMAND", format!("io:threshold-parent {flag}"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        assert_eq!(
            text.contains("QUIET_INFO"),
            flag != "--silent",
            "{flag}: {text}"
        );
        assert!(
            !text.contains("NORMAL_MESSAGE") && !text.contains("HIDDEN_COMPONENT"),
            "{text}"
        );
    }
}

#[test]
fn output_style_accepts_owned_single_pass_string_iterators_without_a_parallel_writer() {
    let input = CommandDefinition::from_signature("iterator")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut source = vec!["借".to_owned(), String::new()].into_iter();
    let mut io = input.io(&mut output);
    io.get_output()
        .writeln_iter(
            std::iter::from_fn(move || source.next()),
            OutputOptions::default(),
        )
        .unwrap();
    assert_eq!(io.get_output().new_lines_written(), 3);
    assert_eq!(io.get_output().get_output().stdout, "借\n\n");
}
