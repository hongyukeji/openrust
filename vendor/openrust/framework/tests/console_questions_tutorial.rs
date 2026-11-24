use openrust::{
    console::{
        AsyncCommandCallback, CommandDefinition, CompletionCallback, ConfirmationQuestion,
        ConsoleError, ConsoleInputStream, Output, OutputStyle, Question, QuestionCallback,
    },
    contracts::{console::Kernel as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "questions:probe {method} {--default=} {--answer=} {--validate} {--tries=2} {--raw} {--multiline} {--timeout=}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut reading = input.clone();
                if let Some(answer) = input
                    .option("answer")
                    .and_then(|value| value.as_str().map(str::to_owned))
                {
                    reading.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(answer.into_bytes()),
                    ))));
                }
                let default = input.option("default").and_then(|value| {
                    value
                        .as_str()
                        .and_then(|value| serde_json::from_str::<Value>(value).ok())
                });
                let mode = input
                    .argument("method")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned();
                let mut question = match mode.as_str() {
                    "confirm" => Question::from(ConfirmationQuestion::new(
                        "Continue",
                        default.as_ref().and_then(Value::as_bool).unwrap_or(false),
                        "/^y/i",
                    )?),
                    "secret" => {
                        let mut question = Question::new("Password", None)?;
                        question.set_hidden(true)?;
                        question
                    }
                    _ => Question::new("Name", default)?,
                };
                question
                    .set_trimmable(input.option("raw") != Some(json!(true)))
                    .set_multiline(input.option("multiline") == Some(json!(true)));
                if let Some(seconds) = input
                    .option("timeout")
                    .and_then(|value| value.as_str().and_then(|text| text.parse::<f64>().ok()))
                {
                    question.set_timeout(Some(
                        std::time::Duration::try_from_secs_f64(seconds)
                            .map_err(ConsoleError::from_source)?,
                    ));
                }
                if input.option("validate") == Some(json!(true)) {
                    question.set_max_attempts(Some(
                        usize::try_from(input.input().integer("tries")).unwrap(),
                    ))?;
                    question.set_validator(Some(QuestionCallback::sync(|value| {
                        if value.as_str() == Some("Ada") {
                            Ok(value)
                        } else {
                            Err(ConsoleError::new("Invalid name"))
                        }
                    })));
                }
                if mode == "complete" {
                    question.set_autocompleter_callback(Some(CompletionCallback::new(
                        |prefix| {
                            Box::pin(async move {
                                tokio::task::yield_now().await;
                                let app = App::get_facade_application()
                                    .map_err(ConsoleError::from_source)?
                                    .ok_or_else(|| ConsoleError::new("没有当前应用"))?;
                                assert!(
                                    app.config()
                                        .map_err(ConsoleError::from_source)?
                                        .get("app.name")
                                        .is_some()
                                );
                                if let Ok(file) = std::env::var("QUESTION_COMPLETION_TRACE") {
                                    use std::io::Write as _;
                                    writeln!(
                                        std::fs::OpenOptions::new()
                                            .create(true)
                                            .append(true)
                                            .open(file)
                                            .map_err(ConsoleError::from_source)?,
                                        "{prefix}"
                                    )
                                    .map_err(ConsoleError::from_source)?;
                                }
                                Ok(vec!["green".into(), "gray".into(), "blue".into()])
                            })
                        },
                    )))?;
                }
                let mut io = reading.io(output);
                let value = io.get_output().ask_question(&question).await?;
                let value = if mode == "chain" {
                    let next = io.get_output().ask_question(&question).await?;
                    json!([value, next])
                } else {
                    value
                };
                let metadata = json!({"value":value,"new_lines":io.get_output().new_lines_written(),"new_line":io.get_output().new_line_written(),"interactive":reading.is_interactive()});
                io.get_output().writeln(
                    metadata.to_string().as_str(),
                    openrust::console::OutputFormat::Raw,
                )?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "questions:helpers",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut io = input.io(output);
                let name = io.ask_with("Name", Some("Guest")).await?;
                let confirmed = io.confirm("Continue").await?;
                let secret = io.secret("Password").await?;
                let completion = io
                    .anticipate(
                        "Color",
                        CompletionCallback::values(vec!["green".into()]),
                        Some("green"),
                    )
                    .await?;
                io.line(&json!({"name":name,"confirmed":confirmed,"secret":secret,"completion":completion}).to_string())?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"name":"Question","key":"a".repeat(32)}),
        ),
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
    ]))
}
fn app(path: &Path) -> Application {
    Application::configure(path.into())
        .with_config(config)
        .with_routing(Routing::default().commands(register).web(routes))
        .create()
        .unwrap()
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/questions/default", || async {
        let mut output = Output::default();
        Artisan::call(
            "questions:helpers",
            &["--no-interaction".into()],
            &mut output,
        )
        .await
        .map_err(|source| openrust::http::HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok::<_, openrust::http::HttpError>(output.stdout)
    })?;
    Route::get("/questions/missing", || async {
        let mut output = Output::default();
        Artisan::call("questions:probe", &["ask".into()], &mut output)
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        Ok::<_, openrust::http::HttpError>(output.stdout)
    })?;
    Ok(())
}
#[tokio::test]
async fn actual_facade_answers_and_styles_match_fixed_question_vectors() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/question-answers-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&application), async {
        for case in
            cases.as_array().unwrap().iter().filter(|case| {
                matches!(case["method"].as_str(), Some("ask" | "confirm" | "secret"))
            })
        {
            let mut args = vec![case["method"].as_str().unwrap().into()];
            if !case["default"].is_null() {
                args.push(format!("--default={}", case["default"]).into());
            }
            if let Some(answer) = case["answer"].as_str() {
                args.push(format!("--answer={answer}\n").into());
            }
            if case["noninteractive"] == true {
                args.push("--no-interaction".into());
            }
            let mut output = Output::default();
            Artisan::call("questions:probe", &args, &mut output)
                .await
                .unwrap();
            let result = output
                .stdout
                .strip_prefix(case["output"].as_str().unwrap())
                .unwrap_or_else(|| panic!("prefix mismatch: {case} actual {:?}", output.stdout));
            let result: Value = serde_json::from_str(result.trim()).unwrap();
            assert_eq!(result["value"], case["value"], "{case}");
            if let Some(count) = case.get("new_lines") {
                assert_eq!(&result["new_lines"], count, "{case}");
            }
        }
    })
    .await;
}
#[tokio::test]
async fn question_type_callbacks_properties_and_conflicts_are_real_public_consumers() {
    let mut question = Question::new("Name", Some(json!("Guest"))).unwrap();
    assert_eq!(question.get_question(), "Name");
    assert_eq!(question.get_default(), Some(&json!("Guest")));
    assert!(!question.is_hidden());
    assert!(question.is_hidden_fallback());
    assert!(question.is_trimmable());
    assert!(!question.is_multiline());
    assert_eq!(question.get_timeout(), None);
    assert_eq!(question.get_max_attempts(), None);
    question
        .set_multiline(true)
        .set_trimmable(false)
        .set_hidden_fallback(false)
        .set_timeout(Some(std::time::Duration::from_secs(3)));
    assert!(question.is_multiline());
    assert!(!question.is_trimmable());
    assert!(!question.is_hidden_fallback());
    assert_eq!(
        question.get_timeout(),
        Some(std::time::Duration::from_secs(3))
    );
    question.set_max_attempts(Some(2)).unwrap();
    assert!(question.set_max_attempts(Some(0)).is_err());
    assert_eq!(question.get_max_attempts(), Some(2));
    question
        .set_autocompleter_values(Some(vec!["alpha".into(), "beta".into()]))
        .unwrap();
    assert_eq!(
        question.get_autocompleter_values().await.unwrap(),
        Some(vec!["alpha".into(), "beta".into()])
    );
    assert!(question.get_autocompleter_callback().is_some());
    assert!(question.set_hidden(false).is_err());
    assert!(!question.is_hidden());
    question
        .set_autocompleter_callback(None)
        .unwrap()
        .set_hidden(true)
        .unwrap();
    assert!(
        question
            .set_autocompleter_values(Some(vec!["unsafe".into()]))
            .is_err()
    );
    assert!(question.is_hidden());
    let seen = Arc::new(AtomicUsize::new(0));
    let calls = seen.clone();
    question.set_normalizer(QuestionCallback::new(move |value| {
        let calls = calls.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        })
    }));
    assert_eq!(
        question
            .get_normalizer()
            .unwrap()
            .call(json!("A"))
            .await
            .unwrap(),
        "A"
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    question.set_validator(Some(QuestionCallback::sync(|value| {
        Ok(json!({"wrapped":value}))
    })));
    assert_eq!(
        question
            .get_validator()
            .unwrap()
            .call(json!("A"))
            .await
            .unwrap(),
        json!({"wrapped":"A"})
    );
    assert!(Question::new("invalid", Some(json!([]))).is_err());
    assert!(Question::new("invalid", Some(json!({}))).is_err());
    let question = ConfirmationQuestion::new("Look ahead", false, "/^(?=y)yes$/i").unwrap();
    assert_eq!(
        question
            .get_normalizer()
            .unwrap()
            .call(json!("YES"))
            .await
            .unwrap(),
        true
    );
    assert_eq!(
        question
            .get_normalizer()
            .unwrap()
            .call(json!("other"))
            .await
            .unwrap(),
        false
    );
}
#[tokio::test]
async fn cancellation_finally_counter_timeout_fallback_and_stream_state_do_not_leak() {
    let (writer, reader) = tokio::io::duplex(32);
    let _writer = writer;
    let mut input = CommandDefinition::from_signature("pending")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        reader,
    ))));
    let cloned = input.clone();
    input.set_interactive(false);
    assert!(cloned.is_interactive());
    input.set_interactive(true);
    let question = Question::new("Pending", None).unwrap();
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            style.ask_question(&question)
        )
        .await
        .is_err()
    );
    assert!(style.get_output().stdout.contains("Pending"));
    assert_eq!(style.new_lines_written(), 1);
    assert!(!style.new_line_written());
    let mut question = Question::new("Fallback", Some(json!("Guest"))).unwrap();
    question.set_timeout(Some(std::time::Duration::from_millis(30)));
    assert_eq!(style.ask_question(&question).await.unwrap(), "Guest");
    assert!(!input.is_interactive());
    assert_eq!(
        style
            .ask_question(&Question::new("Optional", None).unwrap())
            .await
            .unwrap(),
        Value::Null
    );
}
#[test]
#[ignore = "由真实 Artisan 子进程验收调用"]
fn question_fixture() {
    let root = std::env::var("QUESTION_ROOT").unwrap();
    let arguments: Vec<std::ffi::OsString> =
        serde_json::from_str::<Vec<String>>(&std::env::var("QUESTION_ARGUMENTS").unwrap())
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let status = runtime
        .block_on(app(Path::new(&root)).handle_command(&arguments))
        .unwrap();
    drop(runtime);
    std::process::exit(status);
}

#[tokio::test]
async fn supplied_kernel_input_keeps_its_stream_position_interaction_and_shutdown() {
    use openrust::console::Command as _;
    let root = tempfile::tempdir().unwrap();
    let application = Application::configure(root.path().into())
        .with_config(config)
        .create()
        .unwrap();
    let mut commands = openrust::console::Application::new();
    let registered = commands
        .command_with(
            "supplied",
            AsyncCommandCallback::new(|_, input, output| {
                Box::pin(async move {
                    let value = input.io(output).ask_with("Name", Some("Guest")).await?;
                    input
                        .io(output)
                        .line(&format!("VALUE={}", value.unwrap_or_default()))?;
                    Ok(0)
                })
            }),
        )
        .unwrap();
    let definition = registered.definition().clone();
    let mut input = definition.parse(&[]).unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"skipped\nAda\n".to_vec()),
    ))));
    assert!(input.get_stream().is_some());
    {
        use tokio::io::AsyncBufReadExt as _;
        let mut line = String::new();
        input
            .get_stream()
            .unwrap()
            .lock()
            .await
            .read_line(&mut line)
            .await
            .unwrap();
        assert_eq!(line, "skipped\n");
    }
    let mut kernel = openrust::foundation::console::Kernel::new(application, commands);
    let mut output = Output::default();
    kernel
        .call("supplied", &input, Some(&mut output))
        .await
        .unwrap();
    assert!(output.stdout.ends_with("VALUE=Ada\n"));
    assert!(input.is_interactive());
    let mut output = Output::default();
    kernel
        .call("supplied", &input, Some(&mut output))
        .await
        .unwrap();
    assert!(output.stdout.ends_with("VALUE=Guest\n"));
    assert!(!input.is_interactive());
    let mut output = Output::default();
    kernel
        .call("supplied", &input, Some(&mut output))
        .await
        .unwrap();
    assert_eq!(output.stdout, "VALUE=Guest\n");
    let token = tokio_util::sync::CancellationToken::new();
    let mut input = definition.parse(&[]).unwrap().with_shutdown(token.clone());
    let (_writer, reader) = tokio::io::duplex(32);
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        reader,
    ))));
    let cancellation = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        token.cancel();
    });
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        kernel.call("supplied", &input, Some(&mut Output::default())),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .is::<openrust::console::ConsoleQuestionError>()
    );
    cancellation.await.unwrap();
}
#[tokio::test]
async fn real_http_defaults_missing_input_and_recovery_are_isolated() {
    let root = tempfile::tempdir().unwrap();
    use axum::handler::Handler as _;
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
        .unwrap();
    });
    for path in [
        "/questions/default",
        "/questions/missing",
        "/questions/default",
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        if path.ends_with("missing") {
            assert_eq!(response.status(), 500);
            assert!(
                response.headers()["content-type"]
                    .to_str()
                    .unwrap()
                    .contains("application/json")
            );
        } else {
            assert_eq!(response.status(), 200);
            assert_eq!(
                serde_json::from_str::<Value>(&response.text().await.unwrap()).unwrap(),
                json!({"name":"Guest","confirmed":false,"secret":null,"completion":"green"})
            );
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
fn actual_artisan_binary_reads_pipe_answers_and_keeps_quiet_defaults() {
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    for (arguments, answer, expected) in [
        (vec!["questions:probe", "ask"], " Ada\n", json!("Ada")),
        (
            vec!["questions:probe", "confirm", "--default=true"],
            "no\n",
            json!(false),
        ),
        (
            vec!["questions:probe", "secret"],
            "private\n",
            json!("private"),
        ),
    ] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "question_fixture", "--ignored", "--nocapture"])
            .env("QUESTION_ROOT", root.path())
            .env(
                "QUESTION_ARGUMENTS",
                serde_json::to_string(&arguments).unwrap(),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        let line = text
            .lines()
            .find(|line| line.starts_with('{') && line.ends_with('}'))
            .unwrap_or_else(|| panic!("{text:?}"));
        let data: Value = serde_json::from_str(line).unwrap();
        assert_eq!(data["value"], expected);
        if arguments[1] == "secret" {
            assert_eq!(text.matches("private").count(), 1);
        }
    }
    for flag in ["--quiet", "--silent", "-qv"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "question_fixture", "--ignored", "--nocapture"])
            .env("QUESTION_ROOT", root.path())
            .env(
                "QUESTION_ARGUMENTS",
                serde_json::to_string(&["questions:helpers", flag]).unwrap(),
            )
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(
            !text.contains("Name") && !text.contains("Password") && !text.contains("Guest"),
            "{text}"
        );
    }
}

#[derive(Debug)]
struct NameRejected;
impl std::fmt::Display for NameRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid name")
    }
}
impl std::error::Error for NameRejected {}
#[tokio::test]
async fn normalization_async_validation_retries_and_typed_errors_use_the_actual_pipeline() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        let mut input = CommandDefinition::from_signature("validation")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            std::io::Cursor::new(b"BAD\nADA\n".to_vec()),
        ))));
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let calls = seen.clone();
        let mut question = Question::new("Name", None).unwrap();
        question.set_max_attempts(Some(2)).unwrap();
        question.set_normalizer(QuestionCallback::sync(|value| {
            Ok(json!(value.as_str().unwrap().to_lowercase()))
        }));
        question.set_validator(Some(QuestionCallback::new(move |value| {
            let calls = calls.clone();
            Box::pin(async move {
                tokio::task::yield_now().await;
                let app = App::get_facade_application()
                    .map_err(ConsoleError::from_source)?
                    .ok_or_else(|| ConsoleError::new("没有当前应用"))?;
                assert_eq!(
                    app.config()
                        .map_err(ConsoleError::from_source)?
                        .get("app.name"),
                    Some(json!("Question"))
                );
                calls
                    .lock()
                    .unwrap()
                    .push(value.as_str().unwrap().to_owned());
                if value == "ada" {
                    Ok(json!({"name":"Ada"}))
                } else {
                    Err(ConsoleError::from_source(NameRejected))
                }
            })
        })));
        let mut output = Output::default();
        let mut style = OutputStyle::new(&input, &mut output);
        assert_eq!(
            style.ask_question(&question).await.unwrap(),
            json!({"name":"Ada"})
        );
        assert_eq!(*seen.lock().unwrap(), vec!["bad", "ada"]);
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../../.agents/project/audits/artisan-tutorial/question-retry-oracle.json"
        ))
        .unwrap();
        assert_eq!(
            style.get_output().stdout,
            oracle["output"].as_str().unwrap()
        );
        assert_eq!(
            style.new_lines_written() as u64,
            oracle["new_lines"].as_u64().unwrap()
        );
        let mut input = CommandDefinition::from_signature("validation")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            std::io::Cursor::new(b"BAD\n".to_vec()),
        ))));
        question.set_max_attempts(Some(1)).unwrap();
        let mut output = Output::default();
        let error = OutputStyle::new(&input, &mut output)
            .ask_question(&question)
            .await
            .unwrap_err();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .is::<NameRejected>()
        );
        let mut input = CommandDefinition::from_signature("default")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_interactive(false);
        let normalized = Arc::new(AtomicUsize::new(0));
        let calls = normalized.clone();
        let mut question = Question::new("Default", Some(json!("RAW"))).unwrap();
        question.set_normalizer(QuestionCallback::sync(move |value| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        }));
        question.set_validator(Some(QuestionCallback::sync(|value| {
            assert_eq!(value, "RAW");
            Ok(value)
        })));
        assert_eq!(
            OutputStyle::new(&input, &mut Output::default())
                .ask_question(&question)
                .await
                .unwrap(),
            "RAW"
        );
        assert_eq!(normalized.load(Ordering::SeqCst), 0);
    })
    .await;
}

#[tokio::test]
async fn lazy_completion_and_style_validators_keep_public_return_values() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut question = Question::new("Name", None).unwrap();
    question
        .set_autocompleter_iter(["a", "b"].into_iter().map(move |value| {
            count.fetch_add(1, Ordering::SeqCst);
            value.to_owned()
        }))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        question.get_autocompleter_values().await.unwrap(),
        Some(vec!["a".into(), "b".into()])
    );
    assert_eq!(
        question.clone().get_autocompleter_values().await.unwrap(),
        Some(vec!["a".into(), "b".into()])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    question
        .set_autocompleter_pairs(vec![
            ("id".into(), "value".into()),
            ("two".into(), "value".into()),
        ])
        .unwrap();
    assert_eq!(
        question.get_autocompleter_values().await.unwrap(),
        Some(vec![
            "id".into(),
            "two".into(),
            "value".into(),
            "value".into()
        ])
    );
    let definition = CommandDefinition::from_signature("questions:validators").unwrap();
    let mut input = definition.parse(&[]).unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"Ada\nsecret\n".to_vec()),
    ))));
    let mut output = Output::default();
    let mut io = input.io(&mut output);
    let validator = QuestionCallback::sync(|value| Ok(json!({"answer": value})));
    assert_eq!(
        io.get_output()
            .ask_with_validator("Name", None, Some(validator.clone()))
            .await
            .unwrap(),
        json!({"answer":"Ada"})
    );
    assert_eq!(
        io.get_output()
            .ask_hidden_with("Password", Some(validator))
            .await
            .unwrap(),
        json!({"answer":"secret"})
    );
    assert!(!output.stdout.contains("secret"));
}

#[tokio::test]
async fn buffered_answers_keep_full_unicode_lines_delimiters_and_following_answers() {
    let mut input = CommandDefinition::from_signature("answers")
        .unwrap()
        .parse(&[])
        .unwrap();
    let long = "猫😀".repeat(3000);
    input.set_stream(Some(ConsoleInputStream::new(
        tokio::io::BufReader::with_capacity(
            17,
            std::io::Cursor::new(format!("{long}\nnext\n first \nsecond\x04last\n").into_bytes()),
        ),
    )));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let mut question = Question::new("Text", None).unwrap();
    assert_eq!(style.ask_question(&question).await.unwrap(), long);
    assert_eq!(style.ask_question(&question).await.unwrap(), "next");
    question.set_multiline(true).set_trimmable(false);
    assert_eq!(
        style.ask_question(&question).await.unwrap(),
        " first \nsecond"
    );
    question.set_multiline(false).set_trimmable(true);
    assert_eq!(style.ask_question(&question).await.unwrap(), "last");
    question.set_timeout(Some(std::time::Duration::MAX));
    assert!(
        style
            .ask_question(&question)
            .await
            .unwrap_err()
            .to_string()
            .contains("计时器范围")
    );
}
