use openrust::{
    console::{
        AsyncCommandCallback, ChoiceQuestion, ConsoleError, ConsoleInputStream, Output,
        QuestionCallback,
    },
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([(
        "app".into(),
        json!({"name":"Choices","env":"testing","locale":"en","fallback_locale":"en","key":"a".repeat(32)}),
    )]))
}
fn app(root: &Path) -> Application {
    Application::configure(root.into())
        .with_config(config)
        .with_routing(Routing::default().commands(register).web(routes))
        .create()
        .unwrap()
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "choices:probe {case}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let case: Value = serde_json::from_value(input.argument("case").unwrap())
                    .and_then(|text: String| serde_json::from_str(&text))
                    .map_err(ConsoleError::from_source)?;
                let mut reading = input.clone();
                reading.set_interactive(
                    reading.is_interactive() && case["interactive"].as_bool().unwrap(),
                );
                if case["stream"] != false {
                    reading.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(
                            format!("{}\n", case["answer"].as_str().unwrap()).into_bytes(),
                        ),
                    ))));
                }
                let choices = case["choices"].as_array().unwrap().clone();
                let pairs = case.get("pairs").map(|pairs| {
                    pairs
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|pair| (pair[0].as_str().unwrap().to_owned(), pair[1].clone()))
                        .collect::<Vec<_>>()
                });
                let default = case
                    .get("default")
                    .filter(|value| !value.is_null())
                    .cloned();
                let multiple = case["multiple"].as_bool().unwrap();
                let attempts = case["attempts"]
                    .as_u64()
                    .map(|value| usize::try_from(value).unwrap())
                    .unwrap_or(1);
                let (result, new_lines, new_line) = {
                    let mut io = reading.io(output);
                    let result = match case["mode"].as_str().unwrap() {
                        "command" => match pairs {
                            Some(pairs) => {
                                io.choice_pairs_with(
                                    "Color",
                                    pairs,
                                    default,
                                    Some(attempts),
                                    multiple,
                                )
                                .await
                            }
                            None => {
                                io.choice_with("Color", choices, default, Some(attempts), multiple)
                                    .await
                            }
                        },
                        "style" => match pairs {
                            Some(pairs) => {
                                io.get_output()
                                    .choice_pairs_with("Color", pairs, default, multiple)
                                    .await
                            }
                            None => {
                                io.get_output()
                                    .choice_with("Color", choices, default, multiple)
                                    .await
                            }
                        },
                        _ => {
                            let mut question = match pairs {
                                Some(pairs) => ChoiceQuestion::from_pairs("Color", pairs, default)?,
                                None => ChoiceQuestion::new("Color", choices, default)?,
                            };
                            question.set_multiselect(multiple);
                            question.set_max_attempts(Some(attempts))?;
                            question.set_trimmable(case["trim"].as_bool().unwrap());
                            if let Some(error) = case["custom_error"].as_str() {
                                question.set_error_message(error);
                            }
                            if let Some(prompt) = case["prompt"].as_str() {
                                question.set_prompt(prompt);
                            }
                            match case["validator"].as_str() {
                                Some("none") => {
                                    question.set_validator(None);
                                }
                                Some("custom") => {
                                    question.set_validator(Some(QuestionCallback::sync(|value| {
                                        Ok(json!({"custom":value}))
                                    })));
                                }
                                _ => {}
                            }
                            if case["validate_canonical"] == true {
                                let validator = question.get_validator().unwrap().clone();
                                question.set_validator(Some(QuestionCallback::new(move |value| {
                                    let validator = validator.clone();
                                    Box::pin(async move {
                                        tokio::task::yield_now().await;
                                        assert!(
                                            !crossterm::terminal::is_raw_mode_enabled().unwrap(),
                                            "终端原始模式不能泄漏到校验器"
                                        );
                                        validator.call(value).await
                                    })
                                })));
                            }
                            io.get_output().ask_question(question.as_question()).await
                        }
                    };
                    (
                        result,
                        io.get_output().new_lines_written(),
                        io.get_output().new_line_written(),
                    )
                };
                let result = if case["propagate"] == true {
                    Ok(result?)
                } else {
                    result
                };
                let record = match result {
                    Ok(value) => {
                        json!({"value":value,"message":null,"new_lines":new_lines,"new_line":new_line})
                    }
                    Err(error) => {
                        json!({"value":null,"message":error.to_string(),"new_lines":new_lines,"new_line":new_line})
                    }
                };
                output
                    .write_raw(&(record.to_string() + "\n"))
                    .map_err(ConsoleError::from_source)?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "choices:format {case}",
        |input: &openrust::console::Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap())
                    .map_err(ConsoleError::from_source)?;
            let mut io = input.io(output);
            let format = if case["raw"] == true {
                openrust::console::OutputFormat::Raw
            } else {
                openrust::console::OutputFormat::Normal
            };
            io.get_output()
                .write(case["text"].as_str().unwrap(), false, format)?;
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn actual_choice_facade_entries_match_fixed_source_answers_errors_and_rendering() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/choice-answers-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&application), async {
        for (index, case) in cases.iter().enumerate() {
            let mut output = Output::default();
            Artisan::call("choices:probe", &[case.to_string().into()], &mut output)
                .await
                .unwrap();
            let prefix = case["output"].as_str().unwrap();
            let actual = output.stdout.strip_prefix(prefix).unwrap_or_else(|| {
                panic!(
                    "case {index}: prefix mismatch\ncase={case}\nactual={:?}",
                    output.stdout
                )
            });
            let actual: Value = serde_json::from_str(actual.trim()).unwrap();
            assert_eq!(actual["value"], case["value"], "case {index}: {case}");
            assert_eq!(
                actual["message"],
                case.get("message").cloned().unwrap_or(Value::Null),
                "case {index}: {case}"
            );
            assert_eq!(
                actual["new_lines"], case["new_lines"],
                "case {index}: {case}"
            );
            assert_eq!(actual["new_line"], case["new_line"], "case {index}: {case}");
        }
    })
    .await;
}

#[tokio::test]
async fn public_choice_properties_and_retained_validator_follow_the_current_question() {
    let mut question = ChoiceQuestion::new("Color", ["red", "green"], Some(json!(0))).unwrap();
    assert_eq!(question.get_choices()[0].0.to_string(), "0");
    assert_eq!(question.get_choices()[1].1, "green");
    assert_eq!(question.get_prompt(), " > ");
    assert!(!question.is_multiselect());
    question.set_prompt("Pick: ");
    assert_eq!(question.get_prompt(), "Pick: ");
    let retained = question.get_validator().unwrap().clone();
    assert_eq!(retained.call(json!(" green ")).await.unwrap(), "green");
    question.set_trimmable(false);
    assert!(retained.call(json!(" green ")).await.is_err());
    question.set_trimmable(true);
    assert_eq!(retained.call(json!(" green ")).await.unwrap(), "green");
    let mut cloned = question.clone();
    cloned.set_trimmable(false);
    assert!(
        cloned
            .get_validator()
            .unwrap()
            .call(json!(" green "))
            .await
            .is_err()
    );
    assert_eq!(
        question
            .get_validator()
            .unwrap()
            .call(json!(" green "))
            .await
            .unwrap(),
        "green"
    );
    question.set_validator(Some(QuestionCallback::sync(|_| Ok(json!("custom")))));
    assert_eq!(
        question
            .get_validator()
            .unwrap()
            .call(json!("bad"))
            .await
            .unwrap(),
        "custom"
    );
    question.set_multiselect(true);
    assert!(question.is_multiselect());
    assert_eq!(
        question
            .get_validator()
            .unwrap()
            .call(json!("0,1"))
            .await
            .unwrap(),
        json!(["red", "green"])
    );
    question.set_error_message("Bad %s");
    assert_eq!(
        question
            .get_validator()
            .unwrap()
            .call(json!("missing"))
            .await
            .unwrap_err()
            .to_string(),
        "Bad missing"
    );
    assert!(ChoiceQuestion::new("Empty", Vec::<String>::new(), None).is_err());
}

#[tokio::test]
async fn formatted_visible_unicode_matches_source_without_changing_raw_data_or_link_targets() {
    let root = tempfile::tempdir().unwrap();
    let mut application = app(root.path());
    application.boot().await.unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/choice-formatting-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&application), async {
        for case in cases {
            let mut output = Output::default();
            let flag = if case["decorated"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            Artisan::call(
                "choices:format",
                &[case.to_string().into(), flag.into()],
                &mut output,
            )
            .await
            .unwrap();
            assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
        }
    })
    .await;
}

fn base_case() -> Value {
    json!({"mode":"question","choices":["red","green","blue"],"default":null,"answer":"1","multiple":false,"interactive":true,"trim":true})
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, default) in [("/choices/default", "g"), ("/choices/error", "missing")] {
        Route::get(path, move || async move {
            let mut case = base_case();
            case["mode"] = json!("command");
            case["pairs"] = json!([["r", "red"], ["g", "green"]]);
            case["default"] = json!(default);
            case["interactive"] = json!(false);
            case["propagate"] = json!(true);
            let mut output = Output::default();
            Artisan::call("choices:probe", &[case.to_string().into()], &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            let value: Value = serde_json::from_str(&output.stdout).map_err(|source| {
                openrust::http::HttpError::Internal {
                    source: Box::new(source),
                }
            })?;
            openrust::http::Response::json(http::StatusCode::OK, value)
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_choice_defaults_errors_and_recovery_use_the_same_application() {
    use axum::handler::Handler as _;
    use std::sync::Arc;
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
        .unwrap();
    });
    for path in ["/choices/default", "/choices/error", "/choices/default"] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("application/json")
        );
        if path.ends_with("error") {
            assert_eq!(response.status(), 500);
        } else {
            assert_eq!(response.status(), 200);
            assert_eq!(response.json::<Value>().await.unwrap()["value"], "g");
        }
    }
    server.abort();
    let _ = server.await;
}
#[test]
#[ignore = "由实际 Artisan 子进程及 PTY 调用"]
fn choice_fixture() {
    let root = std::env::var("CHOICE_ROOT").unwrap();
    let arguments: Vec<std::ffi::OsString> =
        serde_json::from_str::<Vec<String>>(&std::env::var("CHOICE_ARGUMENTS").unwrap())
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
#[test]
fn actual_artisan_pipe_choices_failures_and_subsequent_consumption() {
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    for (answer, multiple, pairs, expected) in [
        ("2\n", false, None, json!("blue")),
        (
            "g\n",
            false,
            Some(json!([["r", "red"], ["g", "green"]])),
            json!("g"),
        ),
        ("0,2\n", true, None, json!(["red", "blue"])),
    ] {
        let mut case = base_case();
        case["stream"] = json!(false);
        case["multiple"] = json!(multiple);
        if let Some(pairs) = pairs {
            case["pairs"] = pairs;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "choice_fixture", "--ignored", "--nocapture"])
            .env("CHOICE_ROOT", root.path())
            .env(
                "CHOICE_ARGUMENTS",
                serde_json::to_string(&["choices:probe", &case.to_string(), "--no-ansi"]).unwrap(),
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
        assert!(result.status.success(), "{:?}", result);
        let text = String::from_utf8(result.stdout).unwrap();
        let value: Value =
            serde_json::from_str(text.lines().find(|line| line.starts_with('{')).unwrap()).unwrap();
        assert_eq!(value["value"], expected);
    }
    for bad in [true, false] {
        let mut case = base_case();
        case["stream"] = json!(false);
        case["interactive"] = json!(false);
        case["default"] = json!(if bad { "missing" } else { "green" });
        case["propagate"] = json!(true);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "choice_fixture", "--ignored", "--nocapture"])
            .env("CHOICE_ROOT", root.path())
            .env(
                "CHOICE_ARGUMENTS",
                serde_json::to_string(&["choices:probe", &case.to_string(), "--no-ansi"]).unwrap(),
            )
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(if bad { 1 } else { 0 }),
            "{:?}",
            result
        );
        if bad {
            assert!(
                String::from_utf8(result.stderr)
                    .unwrap()
                    .contains("missing")
            );
        }
    }
}

#[tokio::test]
async fn direct_default_helper_entries_and_associative_variants_read_the_same_stream() {
    let mut input = openrust::console::CommandDefinition::from_signature("helpers")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"red\ngreen\nred\ngreen\n".to_vec()),
    ))));
    let mut output = Output::default();
    let mut io = input.io(&mut output);
    assert_eq!(io.choice("Command", ["red", "green"]).await.unwrap(), "red");
    assert_eq!(
        io.get_output()
            .choice("Style", ["red", "green"])
            .await
            .unwrap(),
        "green"
    );
    assert_eq!(
        io.choice_pairs_with(
            "Command map",
            [("r", "red"), ("g", "green")],
            None,
            None,
            false
        )
        .await
        .unwrap(),
        "r"
    );
    assert_eq!(
        io.get_output()
            .choice_pairs_with("Style map", [("r", "red"), ("g", "green")], None, false)
            .await
            .unwrap(),
        "g"
    );
}

#[tokio::test]
async fn error_templates_use_positions_width_precision_and_scalar_casts_through_the_public_validator()
 {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/choice-error-format-oracle.json"
    ))
    .unwrap();
    for (index, case) in cases.iter().enumerate() {
        let mut question =
            ChoiceQuestion::from_pairs("Color", [("r", "red"), ("g", "green")], None).unwrap();
        question.set_trimmable(false);
        question.set_error_message(case["template"].as_str().unwrap());
        let error = question
            .get_validator()
            .unwrap()
            .call(case["input"].clone())
            .await
            .unwrap_err();
        let kind = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<openrust::console::ConsoleQuestionError>()
            .unwrap_or_else(|| panic!("case {index}: {case}, error {error:?}"));
        if !case["error"].is_null() {
            assert!(
                matches!(
                    kind,
                    openrust::console::ConsoleQuestionError::InvalidConfiguration(_)
                ),
                "case {index}: {case} error {kind:?}"
            );
        } else if let Some(output) = case["output"].as_str() {
            assert!(
                matches!(
                    kind,
                    openrust::console::ConsoleQuestionError::InvalidAnswer(_)
                ),
                "case {index}: {case} error {kind:?}"
            );
            assert_eq!(error.to_string(), output, "case {index}: {case}");
        } else {
            assert!(
                matches!(
                    kind,
                    openrust::console::ConsoleQuestionError::InvalidEncoding(_)
                ),
                "case {index}: {case} error {kind:?}"
            );
            assert!(
                std::error::Error::source(kind)
                    .unwrap()
                    .downcast_ref::<std::string::FromUtf8Error>()
                    .is_some()
            );
        }
    }
}

#[tokio::test]
async fn operational_and_template_errors_abort_retries_while_missing_input_uses_default() {
    use openrust::console::{CommandDefinition, ConsoleQuestionError, OutputStyle, Question};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::io::AsyncWriteExt as _;
    for mode in ["runtime", "io", "template", "dump"] {
        let (mut writer, reader) = tokio::io::duplex(64);
        writer.write_all(b"bad\n").await.unwrap();
        let mut input = CommandDefinition::from_signature("fatal")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            reader,
        ))));
        let mut output = Output::default();
        let mut question = ChoiceQuestion::new("Color", ["red", "green"], None).unwrap();
        question.set_error_message("Bad %2$s");
        let validator = question.get_validator().unwrap().clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        question.set_validator(Some(QuestionCallback::new(move |value| {
            let calls = calls.clone();
            let validator = validator.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                match mode {
                    "dump" => Err(ConsoleError::from(
                        openrust::support::CollectionError::DumpAndDie("dumped".into()),
                    )),
                    "runtime" => Err(ConsoleError::from_runtime_source(ConsoleError::new(
                        "unavailable",
                    ))),
                    "io" => Err(ConsoleError::from_runtime_source(std::io::Error::other(
                        "unavailable",
                    ))),
                    _ => validator.call(value).await,
                }
            })
        })));
        let mut style = OutputStyle::new(&input, &mut output);
        let error = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            style.ask_question(question.as_question()),
        )
        .await
        .expect("运行错误不能等待第二次回答")
        .unwrap_err();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(input.is_interactive());
        assert_eq!(output.stdout.matches("Color").count(), 1);
        if mode == "runtime" {
            assert!(error.is_runtime());
            assert!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<ConsoleError>()
                    .is_some()
            );
        }
        if mode == "io" {
            assert!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .is_some()
            );
        }
    }
    let mut input = CommandDefinition::from_signature("fallback")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"bad\n".to_vec()),
    ))));
    let mut question = Question::new("Name", Some(json!("Guest"))).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    question.set_validator(Some(QuestionCallback::sync(move |value| {
        calls.fetch_add(1, Ordering::SeqCst);
        if value == "bad" {
            Err(ConsoleError::from_source(
                ConsoleQuestionError::MissingInput,
            ))
        } else {
            Ok(value)
        }
    })));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    assert_eq!(style.ask_question(&question).await.unwrap(), "Guest");
    assert!(!input.is_interactive());
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ordinary_callback_sources_keep_retry_behavior_and_original_causes() {
    use openrust::console::{CommandDefinition, OutputStyle, Question};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut input = CommandDefinition::from_signature("ordinary")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"bad\n".to_vec()),
    ))));
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut question = Question::new("Name", Some(json!("Guest"))).unwrap();
    question.set_validator(Some(QuestionCallback::sync(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(ConsoleError::from_source(std::io::Error::other(
            "unavailable",
        )))
    })));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let error = style.ask_question(&question).await.unwrap_err();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    assert!(!input.is_interactive());
    assert!(!error.is_runtime());
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .is_some()
    );
}

#[tokio::test]
async fn display_only_labels_work_through_the_actual_associative_command_helper() {
    struct Label(&'static str);
    impl std::fmt::Display for Label {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }
    let mut input = openrust::console::CommandDefinition::from_signature("labels")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"green\n".to_vec()),
    ))));
    let mut output = Output::default();
    let mut io = input.io(&mut output);
    let selected = io
        .choice_pairs_with(
            "Color",
            [
                ("r", openrust::console::ChoiceLabel(Label("red"))),
                ("g", openrust::console::ChoiceLabel(Label("green"))),
            ],
            None,
            Some(1),
            false,
        )
        .await
        .unwrap();
    assert_eq!(selected, "g");
    assert!(output.stdout.contains("[r] red"));
    assert!(output.stdout.contains("[g] green"));
}
