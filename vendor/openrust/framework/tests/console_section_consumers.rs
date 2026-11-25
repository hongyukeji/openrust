use openrust::console::{
    CommandDefinition, ConsoleError, Output, OutputFormatter, OutputOptions, OutputVerbosity,
    ProgressBar,
};
use serde_json::Value;
#[test]
fn selected_progress_matches_source_in_isolated_process() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "section_progress_driver",
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
#[ignore = "独立父消费者驱动，隔离终端环境变量"]
fn section_progress_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/section-progress-oracle.json"
    ))
    .unwrap();
    let input = CommandDefinition::from_signature("sections")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        unsafe {
            std::env::set_var("COLUMNS", case["width"].to_string());
        }
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        output.set_verbosity(if case["level"] == 16 {
            OutputVerbosity::Quiet
        } else {
            OutputVerbosity::Normal
        });
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default()).unwrap();
            section.handle()
        };
        let later = {
            let mut section = output.section();
            section.writeln("later", OutputOptions::default()).unwrap();
            section.handle()
        };
        let result = (|| -> Result<i64, ConsoleError> {
            let mut section = output.section_output(&first)?;
            let mut style = section.style(&input);
            let mut bar = if case["ctor"] == "style" {
                style.create_progress_bar(3)?
            } else {
                ProgressBar::new(style.get_output(), 3)?
            };
            bar.set_format(case["format"].as_str().unwrap())
                .set_message("step");
            bar.min_seconds_between_redraws(0.0)
                .max_seconds_between_redraws(0.0)
                .set_redraw_frequency(Some(1));
            if let Some(overwrite) = case["overwrite"].as_bool() {
                bar.set_overwrite(overwrite);
            }
            bar.start()?;
            bar.advance()?;
            bar.display()?;
            bar.get_output()
                .writeln("notice", OutputOptions::default())?;
            if case["ctor"] == "getter" {
                bar.get_output().set_verbosity(OutputVerbosity::Debug);
                bar.get_output()
                    .set_formatter(std::sync::Arc::new(OutputFormatter::new(false)));
            }
            bar.advance()?;
            bar.clear()?;
            bar.display()?;
            bar.finish()?;
            Ok(bar.get_progress())
        })();
        let section = output.section_output(&first).unwrap();
        let content = section.get_content();
        let visible = section.get_visible_content();
        if result.is_err() == case["error"].is_null()
            || output.stdout != case["output"].as_str().unwrap()
            || content != case["content"].as_str().unwrap()
            || visible != case["visible"].as_str().unwrap()
        {
            failures.push(format!("case {index}: result {result:?}, actual {:?}, expected {:?}, content {:?}, expected {:?}",output.stdout,case["output"],content,case["content"]));
        }
        assert_eq!(
            output.section_output(&later).unwrap().get_content(),
            case["later"].as_str().unwrap()
        );
    }
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(question_source_cases());
}
async fn question_source_cases() {
    use openrust::console::{ConfirmationQuestion, ConsoleInputStream, Question, QuestionCallback};
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/section-questions-oracle.json"
    ))
    .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        unsafe {
            std::env::set_var("COLUMNS", case["width"].to_string());
        }
        let mut input = CommandDefinition::from_signature("question")
            .unwrap()
            .parse(&[])
            .unwrap();
        input
            .set_interactive(case["interactive"].as_bool().unwrap())
            .set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                std::io::Cursor::new(case["answer"].as_str().unwrap().as_bytes().to_vec()),
            ))));
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default()).unwrap();
            section.handle()
        };
        {
            let mut later = output.section();
            later.writeln("later", OutputOptions::default()).unwrap();
        }
        let mut question = if case["kind"] == "confirm" {
            ConfirmationQuestion::new("Continue", case["default"].as_bool().unwrap(), "/^y/i")
                .unwrap()
                .into()
        } else {
            Question::new(
                "Name",
                case.get("default").filter(|v| !v.is_null()).cloned(),
            )
            .unwrap()
        };
        if case["kind"] == "hidden" {
            question.set_hidden(true).unwrap();
        }
        if case["kind"] == "retry" {
            question.set_max_attempts(Some(2)).unwrap();
            question.set_validator(Some(QuestionCallback::sync(|value| {
                if value == "Ada" {
                    Ok(value)
                } else {
                    Err(ConsoleError::new("retry"))
                }
            })));
        }
        let mut section = output.section_output(&first).unwrap();
        let result = {
            let mut style = section.style(&input);
            style.ask_question(&question).await
        };
        let before = section.get_content();
        section.clear(None).unwrap();
        section.writeln("after", OutputOptions::default()).unwrap();
        let content = section.get_content();
        let same_result = match result {
            Ok(value) => case["error"].is_null() && value == case["value"],
            Err(_) => !case["error"].is_null(),
        };
        if !same_result
            || output.stdout != case["output"].as_str().unwrap()
            || before != case["before_clear"].as_str().unwrap()
            || content != case["content"].as_str().unwrap()
            || input.is_interactive() != case["interactive_after"].as_bool().unwrap()
        {
            failures.push(format!(
                "case {index}: actual {:?}, expected {:?}, before {:?} expected {:?}",
                output.stdout, case["output"], before, case["before_clear"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} question failures\n{}",
        failures.len(),
        failures.join("\n")
    );
}
use openrust::{
    console::{OutputStyle, Question},
    foundation::{Application, configuration::Routing},
    support::facades::Artisan,
};
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "sections:consumer {mode}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input.argument("mode").unwrap().as_str().unwrap().to_owned();
                if mode == "bad" {
                    let mut style = OutputStyle::new(input, output);
                    style.get_output().as_section()?;
                    return Ok(0);
                }
                let first = {
                    let mut section = output.section();
                    section.writeln("prefix", OutputOptions::default())?;
                    section.handle()
                };
                {
                    let mut later = output.section();
                    later.writeln("later", OutputOptions::default())?;
                }
                let mut section = output.section_output(&first)?;
                if mode == "question" {
                    {
                        let mut style = section.style(input);
                        assert_eq!(
                            style.ask_question(&Question::new("Name", None)?).await?,
                            serde_json::json!("Ada")
                        );
                    }
                    let before = section.get_content();
                    section.clear(None)?;
                    section.writeln("after", OutputOptions::default())?;
                    eprintln!(
                        "SECTION_META:{}",
                        serde_json::json!({"before":before,"after":section.get_content()})
                    );
                } else {
                    let mut style = OutputStyle::new(input, &mut section);
                    let mut bar = ProgressBar::new(style.get_output(), 3)?;
                    bar.set_format("<info>%current%/%max%</info>");
                    bar.set_message("step");
                    bar.min_seconds_between_redraws(0.0)
                        .max_seconds_between_redraws(0.0)
                        .set_redraw_frequency(Some(1));
                    bar.start()?;
                    bar.advance()?;
                    bar.display()?;
                    bar.get_output()
                        .writeln("notice", OutputOptions::default())?;
                    bar.advance()?;
                    bar.clear()?;
                    bar.display()?;
                    bar.finish()?;
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由真实Artisan及PTY父消费者驱动"]
fn section_consumer_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::env::var_os("SECTION_CONSUMER_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let args = [
        "sections:consumer".into(),
        std::env::var_os("SECTION_CONSUMER_MODE").unwrap(),
        std::env::var_os("SECTION_CONSUMER_ANSI").unwrap_or_else(|| "--no-ansi".into()),
    ];
    std::process::exit(runtime.block_on(app.handle_command(&args)).unwrap());
}
fn progress_formal(decorated: bool) -> String {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/section-progress-oracle.json"
    ))
    .unwrap();
    cases
        .iter()
        .find(|c| {
            c["decorated"] == decorated
                && c["width"] == 80
                && c["level"] == 32
                && c["ctor"] == "manual"
                && c["overwrite"].is_null()
                && c["format"] == "<info>%current%/%max%</info>"
        })
        .unwrap()["output"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[test]
fn real_artisan_section_progress_and_unselected_getter_exit() {
    for mode in ["progress", "bad"] {
        for decorated in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "section_consumer_cli_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SECTION_CONSUMER_ROOT", root.path())
                .env("SECTION_CONSUMER_MODE", mode)
                .env(
                    "SECTION_CONSUMER_ANSI",
                    if decorated { "--ansi" } else { "--no-ansi" },
                )
                .env("COLUMNS", "80")
                .output()
                .unwrap();
            assert_eq!(
                child.status.code(),
                Some(if mode == "bad" { 1 } else { 0 }),
                "{}",
                String::from_utf8_lossy(&child.stderr)
            );
            let stdout = String::from_utf8(child.stdout).unwrap();
            assert_eq!(
                stdout.strip_prefix("\nrunning 1 test\n").unwrap(),
                if mode == "bad" {
                    String::new()
                } else {
                    progress_formal(decorated)
                }
            );
            if mode == "bad" {
                assert!(String::from_utf8_lossy(&child.stderr).contains("分区"));
            } else {
                assert!(child.stderr.is_empty());
            }
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, mode) in [
        ("/section-consumers", "progress"),
        ("/section-consumers/error", "bad"),
    ] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            Artisan::call(
                "sections:consumer",
                &[mode.into(), "--no-ansi".into()],
                &mut output,
            )
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                serde_json::json!({"progress":output.stdout}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_selected_progress_and_target_error_recover() {
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
        .unwrap();
    });
    for (path, status) in [
        ("/section-consumers", 200),
        ("/section-consumers/error", 500),
        ("/section-consumers", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["progress"],
                progress_formal(false)
            );
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn replaced_section_formatter_is_used_by_table_titles_widths_and_orientations() {
    use openrust::console::{OutputFormatterStyle, Table};
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/section-table-oracle.json"
    ))
    .unwrap();
    let input = CommandDefinition::from_signature("table")
        .unwrap()
        .parse(&[])
        .unwrap();
    for (index, case) in cases.iter().enumerate() {
        let mut output = Output::default();
        let decorated = case["decorated"].as_bool().unwrap();
        output.set_decorated(decorated);
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default()).unwrap();
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("later", OutputOptions::default()).unwrap();
        }
        let mut section = output.section_output(&first).unwrap();
        let formatter = std::sync::Arc::new(OutputFormatter::new(decorated));
        formatter.set_style(
            "tone",
            std::sync::Arc::new(OutputFormatterStyle::new(Some("red"), None, &[]).unwrap()),
        );
        let mut style = OutputStyle::new(&input, &mut section);
        style.get_output().set_formatter(formatter);
        assert!(style.get_output().get_formatter().has_style("tone"));
        let mut table = Table::new(&mut style).unwrap();
        table
            .set_header_title(Some("<tone>Title</tone>"))
            .set_headers(["<tone>Heading</tone>", "B"])
            .set_rows([
                ["<tone>long text wraps</tone>", "value"],
                ["<tone>one\ntwo</tone>", "next"],
            ]);
        match case["mode"].as_str().unwrap() {
            "horizontal" => {
                table.set_horizontal(true);
            }
            "vertical" => {
                table.set_vertical(true);
            }
            _ => {}
        }
        if case["wrap"] == true {
            table.set_column_max_width(0, 8).unwrap();
        }
        let result = table.render();
        assert_eq!(result.is_err(), !case["error"].is_null());
        drop(table);
        drop(style);
        assert_eq!(
            section.get_content(),
            case["content"].as_str().unwrap(),
            "content case {index}"
        );
        assert_eq!(
            output.stdout,
            case["output"].as_str().unwrap(),
            "output case {index}"
        );
    }
}
#[tokio::test]
async fn cancelling_a_section_question_keeps_target_identity_and_releases_borrows() {
    use openrust::console::{ConsoleInputStream, Question};
    let (writer, reader) = tokio::io::duplex(16);
    let _writer = writer;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let mut input = CommandDefinition::from_signature("question")
        .unwrap()
        .parse(&[])
        .unwrap()
        .with_shutdown(shutdown.clone());
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        reader,
    ))));
    let mut output = Output::default();
    output.set_decorated(true);
    let handle = output.section().handle();
    let mut section = output.section_output(&handle).unwrap();
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        shutdown.cancel();
    });
    let result = {
        let mut style = section.style(&input);
        style
            .ask_question(&Question::new("Pending", None).unwrap())
            .await
    };
    cancel.await.unwrap();
    assert!(result.is_err());
    assert!(section.get_content().contains("Pending"));
    section.clear(None).unwrap();
    section
        .writeln("recovered", OutputOptions::default())
        .unwrap();
    assert_eq!(section.get_content(), "recovered\n");
}
#[test]
fn existing_mutable_stream_parameters_remain_reusable_after_constructor_borrows() {
    fn consumer(input: &openrust::console::Input, output: &mut Output) -> Result<(), ConsoleError> {
        {
            let mut bar = ProgressBar::new(output, 1)?;
            bar.set_format("plain");
            bar.start()?;
            bar.finish()?;
        }
        output.info("after bar");
        {
            let mut style = OutputStyle::new(input, output);
            style.writeln("styled", OutputOptions::default())?;
        }
        output.comment("after style");
        Ok(())
    }
    let input = CommandDefinition::from_signature("borrow")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    consumer(&input, &mut output).unwrap();
    assert!(
        output.stdout.contains("plain")
            && output.stdout.contains("styled")
            && output.stdout.contains("after style")
    );
}
