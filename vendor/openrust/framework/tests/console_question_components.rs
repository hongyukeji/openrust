use openrust::console::{
    CommandDefinition, ConsoleError, ConsoleInputStream, Output, OutputStyle, OutputVerbosity,
    QuestionHelperBehavior,
    view::components::{Ask, AskWithCompletion, Choice, Confirm, Secret},
};
use serde_json::Value;
use std::sync::Arc;
async fn invoke(style: &mut OutputStyle<'_>, case: &Value) -> Result<Value, ConsoleError> {
    let label = case["label"].as_str().unwrap();
    let default = case.get("default").filter(|v| !v.is_null()).cloned();
    match case["kind"].as_str().unwrap() {
        "Ask" => {
            if default.is_none() && case["multiline"] == false {
                Ask::new(style).render(label).await
            } else {
                Ask::new(style)
                    .render_with(label, default, case["multiline"].as_bool().unwrap())
                    .await
            }
        }
        "AskWithCompletion" => {
            let choices = case["choices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap());
            if default.is_none() {
                AskWithCompletion::new(style).render(label, choices).await
            } else {
                AskWithCompletion::new(style)
                    .render_with(label, choices, default)
                    .await
            }
        }
        "Confirm" => {
            if case["default"] == false {
                Confirm::new(style).render(label).await.map(Value::Bool)
            } else {
                Confirm::new(style)
                    .render_with(label, true)
                    .await
                    .map(Value::Bool)
            }
        }
        "Secret" => {
            if case["fallback"] == false {
                Secret::new(style).render_with(label, false).await
            } else {
                Secret::new(style).render(label).await
            }
        }
        "Choice" => {
            let attempts = case["attempts"]
                .as_u64()
                .map(|v| usize::try_from(v).unwrap());
            let multiple = case["multiple"].as_bool().unwrap();
            if let Some(values) = case["choices"].as_array() {
                if default.is_none() && attempts.is_none() && !multiple {
                    Choice::new(style)
                        .render(label, values.iter().cloned())
                        .await
                } else {
                    Choice::new(style)
                        .render_with(label, values.iter().cloned(), default, attempts, multiple)
                        .await
                }
            } else {
                Choice::new(style)
                    .render_pairs_with(
                        label,
                        case["choices"]
                            .as_object()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| (k.as_str(), v.clone())),
                        default,
                        attempts,
                        multiple,
                    )
                    .await
            }
        }
        _ => unreachable!(),
    }
}
#[test]
#[ignore = "由父消费者设置真实COLUMNS并调用全部组件类型"]
fn components_matrix_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/question-components-oracle.json"
    ))
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = openrust::foundation::Application::configure(root.path().into())
        .create()
        .unwrap();
    rt.block_on(async {
        app.boot().await.unwrap();
        use openrust::support::facades::{App, Facade as _};
        App::set_facade_application(Some(&app), async {
            for case in &cases {
                let mut input = CommandDefinition::from_signature("components:questions")
                    .unwrap()
                    .parse(&[])
                    .unwrap();
                input
                    .set_interactive(case["interactive"].as_bool().unwrap())
                    .set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(case["input"].as_str().unwrap().as_bytes().to_vec()),
                    ))));
                let mut output = Output::default();
                output.set_decorated(case["decorated"].as_bool().unwrap());
                output.set_verbosity(match case["level"].as_u64().unwrap() {
                    8 => OutputVerbosity::Silent,
                    16 => OutputVerbosity::Quiet,
                    64 => OutputVerbosity::Verbose,
                    _ => OutputVerbosity::Normal,
                });
                let mut style = OutputStyle::new(&input, &mut output);
                assert!(style.get_question_helper().is_none());
                let result = invoke(&mut style, case).await;
                assert_eq!(
                    result.is_err(),
                    !case["error"].is_null(),
                    "{case}: {result:?}"
                );
                if let Ok(answer) = result {
                    assert_eq!(answer, case["answer"], "{case}");
                }
                assert!(
                    style.get_question_helper().is_some(),
                    "helper not restored: {case}"
                );
                assert_eq!(
                    style.new_lines_written(),
                    case["new_lines"].as_u64().unwrap() as usize,
                    "{case}"
                );
                assert_eq!(output.stdout, case["output"], "{case}");
                assert!(output.stderr.is_empty());
                assert_eq!(input.is_interactive(), case["interactive_after"], "{case}");
            }
        })
        .await;
    });
    println!("COMPONENT_QUESTION_CASES_OK:{}", cases.len());
}
#[test]
fn all_five_component_render_entries_match_fixed_source() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "components_matrix_driver",
            "--ignored",
            "--nocapture",
        ])
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
    assert!(String::from_utf8_lossy(&result.stdout).contains("COMPONENT_QUESTION_CASES_OK:134"));
}
struct Previous;
impl QuestionHelperBehavior for Previous {
    fn get_name(&self) -> &str {
        "previous"
    }
}
#[tokio::test]
async fn component_failures_restore_custom_helper_and_sparse_policy_keeps_base_choice_unchanged() {
    use openrust::console::{ChoiceKey, ChoiceQuestion};
    use serde_json::json;
    let mut input = CommandDefinition::from_signature("components:policy")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_interactive(false);
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let previous: Arc<dyn QuestionHelperBehavior> = Arc::new(Previous);
    style.set_question_helper(Some(previous.clone()));
    assert!(
        Choice::new(&mut style)
            .render("Empty", Vec::<String>::new())
            .await
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &previous,
        &style.get_question_helper().unwrap()
    ));
    assert!(
        Choice::new(&mut style)
            .render_with("Invalid", ["First"], None, Some(0), false)
            .await
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &previous,
        &style.get_question_helper().unwrap()
    ));
    let choice = Choice::new(&mut style);
    assert!(choice.is_assoc(&[
        (ChoiceKey::Integer(1), json!("First")),
        (ChoiceKey::Integer(3), json!("Third"))
    ]));
    assert!(!choice.is_assoc(&[
        (ChoiceKey::Integer(0), json!("First")),
        (ChoiceKey::Integer(1), json!("Second"))
    ]));
    let component = choice
        .get_choice_question_pairs(
            "Sparse",
            [(1_i64, "First"), (3_i64, "Third")],
            Some(json!(1)),
        )
        .unwrap();
    let base = ChoiceQuestion::from_pairs(
        "Sparse",
        [(1_i64, "First"), (3_i64, "Third")],
        Some(json!(1)),
    )
    .unwrap();
    let component_answer = style.ask_question(component.as_question()).await.unwrap();
    let base_answer = style.ask_question(base.as_question()).await.unwrap();
    assert_eq!(component_answer, json!("1"));
    assert_eq!(base_answer, json!("First"));
}
#[tokio::test]
async fn associative_override_uses_fixed_source_validator_and_completion_snapshots() {
    use openrust::console::{ChoiceKey, ChoiceQuestion};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let source: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/choice-association-oracle.json"
    ))
    .unwrap();
    let policy = Arc::new(AtomicBool::new(false));
    let shared = policy.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let invoked = calls.clone();
    let mut question = ChoiceQuestion::from_pairs_with_associative(
        "Policy",
        [(0_i64, "First"), (0_i64, "First"), (1_i64, "Second")],
        Some(json!(0)),
        move |entries| {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].0, ChoiceKey::Integer(0));
            assert_eq!(entries[1].0, ChoiceKey::Integer(1));
            invoked.fetch_add(1, Ordering::SeqCst);
            shared.load(Ordering::SeqCst)
        },
    )
    .unwrap();
    let mut trace =
        vec![json!({"event":"constructed","calls":calls.load(Ordering::SeqCst),"answer":null})];
    policy.store(true, Ordering::SeqCst);
    let mut input = CommandDefinition::from_signature("choice:policy")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_interactive(false);
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let answer = style.ask_question(question.as_question()).await.unwrap();
    trace.push(json!({"event":"answer_before_rebuild","calls":calls.load(Ordering::SeqCst),"answer":answer}));
    question.set_multiselect(false);
    trace.push(json!({"event":"multiselect","calls":calls.load(Ordering::SeqCst),"answer":null}));
    let answer = style.ask_question(question.as_question()).await.unwrap();
    trace.push(json!({"event":"answer_after_rebuild","calls":calls.load(Ordering::SeqCst),"answer":answer}));
    question.set_error_message("%s invalid");
    trace.push(json!({"event":"error_message","calls":calls.load(Ordering::SeqCst),"answer":null}));
    let copy = question.clone();
    trace.push(json!({"event":"clone","calls":calls.load(Ordering::SeqCst),"answer":null}));
    let answer = style.ask_question(copy.as_question()).await.unwrap();
    trace
        .push(json!({"event":"clone_answer","calls":calls.load(Ordering::SeqCst),"answer":answer}));
    assert_eq!(json!(trace), source["trace"]);
    assert_eq!(
        question
            .get_autocompleter_callback()
            .unwrap()
            .call(String::new())
            .await
            .unwrap(),
        serde_json::from_value::<Vec<String>>(source["completion"].clone()).unwrap()
    );
    let component = Choice::new(&mut style);
    let built = component
        .get_choice_question("List", ["First", "Second"], None)
        .unwrap();
    assert_eq!(built.get_choices().len(), 2);
}
async fn run_default(
    style: &mut (impl openrust::console::QuestionStyle + ?Sized),
    kind: &str,
    callback: bool,
    invalid: bool,
) -> Result<Value, ConsoleError> {
    use openrust::console::CompletionCallback;
    match kind {
        "Ask" => Ask::new(style).render("Name").await,
        "AskMultiline" => Ask::new(style).render_with("Name", None, true).await,
        "AskWithCompletion" if callback => {
            AskWithCompletion::new(style)
                .render_with_callback(
                    "Name",
                    CompletionCallback::sync(|typed| {
                        if let Ok(path) = std::env::var("COMPONENT_TRACE") {
                            use std::io::Write as _;
                            let mut file = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(path)
                                .map_err(ConsoleError::from_source)?;
                            writeln!(file, "{}", serde_json::to_string(&typed).unwrap())
                                .map_err(ConsoleError::from_source)?;
                        }
                        if typed == "err" {
                            return Err(ConsoleError::new("completion failed"));
                        }
                        Ok(vec!["green".into(), "gray".into()])
                    }),
                    None,
                )
                .await
        }
        "AskWithCompletion" => {
            AskWithCompletion::new(style)
                .render("Name", ["green", "gray"])
                .await
        }
        "Confirm" => Confirm::new(style)
            .render("Continue")
            .await
            .map(Value::Bool),
        "Secret" => Secret::new(style).render("Password").await,
        "SecretStrict" => Secret::new(style).render_with("Password", false).await,
        "Choice" if invalid => {
            Choice::new(style)
                .render("Color", Vec::<String>::new())
                .await
        }
        "Choice" => Choice::new(style).render("Color", ["red", "green"]).await,
        _ => unreachable!(),
    }
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:show {--kind=Ask} {--surface=derived} {--answer=} {--callback} {--invalid}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                use openrust::console::{AsOutput, ConsoleStyle, OutputFormat};
                let mut input = input.clone();
                if let Some(answer) = input
                    .option("answer")
                    .and_then(|v| v.as_str().map(str::to_owned))
                {
                    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(answer.into_bytes()),
                    ))));
                }
                let kind = input
                    .option("kind")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Ask".into());
                let surface = input
                    .option("surface")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "derived".into());
                let callback = input.option("callback").and_then(|v| v.as_bool()) == Some(true);
                let invalid = input.option("invalid").and_then(|v| v.as_bool()) == Some(true);
                let value = match surface.as_str() {
                    "base" => {
                        run_default(
                            &mut ConsoleStyle::new(&input, output),
                            &kind,
                            callback,
                            invalid,
                        )
                        .await?
                    }
                    "nested" => {
                        let mut parent = OutputStyle::new(&input, output);
                        run_default(
                            &mut OutputStyle::new(&input, &mut parent),
                            &kind,
                            callback,
                            invalid,
                        )
                        .await?
                    }
                    "stderr" => {
                        let mut style = OutputStyle::new(&input, output);
                        run_default(&mut style.get_error_style(), &kind, callback, invalid).await?
                    }
                    _ => {
                        run_default(
                            &mut OutputStyle::new(&input, output),
                            &kind,
                            callback,
                            invalid,
                        )
                        .await?
                    }
                };
                output.as_output().writeln(
                    format!("COMPONENT_VALUE:{}", serde_json::to_string(&value).unwrap()).as_str(),
                    OutputFormat::Raw,
                )?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由实际Artisan/PTY父消费者设置参数、输入和输出"]
fn components_cli_driver() {
    let case: Value = serde_json::from_str(&std::env::var("COMPONENT_CASE").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let mut args: Vec<std::ffi::OsString> = vec![
        "components:show".into(),
        format!("--kind={}", case["kind"].as_str().unwrap()).into(),
        format!("--surface={}", case["surface"].as_str().unwrap()).into(),
        if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        }
        .into(),
    ];
    if case["callback"] == true {
        args.push("--callback".into());
    }
    if case["invalid"] == true {
        args.push("--invalid".into());
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn all_component_classes_in_real_artisan_match_source_physical_streams() {
    use std::io::Write as _;
    let rows:Vec<Value>=serde_json::from_str(include_str!("../../../../.agents/project/audits/artisan-tutorial/question-components-console-oracle.json")).unwrap();
    for case in rows {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "components_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("COLUMNS", "80")
            .env("COMPONENT_CASE", case.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(case["input"].as_str().unwrap().as_bytes())
            .unwrap();
        let result = process.wait_with_output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "{case}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stdout = stdout.strip_prefix("\nrunning 1 test\n").unwrap_or(&stdout);
        assert_eq!(stdout, case["stdout"], "{case}");
        assert_eq!(
            String::from_utf8(result.stderr).unwrap(),
            case["stderr"],
            "{case}"
        );
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/components", component_response)?;
    Ok(())
}
fn component_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let kind = request
            .query(Some("kind"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Ask".into());
        let answer = request
            .query(Some("answer"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Guest".into());
        let mut args = vec![
            format!("--kind={kind}").into(),
            format!("--answer={answer}\n").into(),
        ];
        if request.query(Some("invalid"))?.is_some() {
            args.push("--invalid".into());
        }
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("components:show", &args, &mut output).await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            serde_json::json!({"output":output.stdout,"error":output.stderr}),
        )
    })
}
#[tokio::test]
async fn actual_http_component_types_errors_recovery_and_parallel_commands() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
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
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    for (kind, answer, expected) in [
        ("Ask", "reader", serde_json::json!("reader")),
        ("AskWithCompletion", "green", serde_json::json!("green")),
        ("Choice", "1", serde_json::json!("green")),
        ("Confirm", "yes", serde_json::json!(true)),
        ("Secret", "secret", serde_json::json!("secret")),
    ] {
        let response = reqwest::get(format!(
            "http://{address}/components?kind={kind}&answer={answer}"
        ))
        .await
        .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert!(
            body["output"]
                .as_str()
                .unwrap()
                .contains(&format!("COMPONENT_VALUE:{expected}")),
            "{body}"
        );
    }
    let response = reqwest::get(format!("http://{address}/components?kind=Choice&invalid=1"))
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..12 {
        requests.spawn(async move {
            let answer = format!("Recovered{i}");
            let response = reqwest::get(format!(
                "http://{address}/components?kind=Ask&answer={answer}"
            ))
            .await
            .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("COMPONENT_VALUE:\"{answer}\""))
            );
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn cancellation_of_each_component_restores_identity_and_releases_original_input() {
    use tokio::io::AsyncWriteExt as _;
    let source: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/question-components-oracle.json"
    ))
    .unwrap();
    for kind in ["Ask", "AskWithCompletion", "Choice", "Confirm", "Secret"] {
        let case = source
            .iter()
            .find(|case| {
                case["kind"] == kind
                    && case["interactive"] == true
                    && !case["input"].as_str().unwrap().is_empty()
            })
            .unwrap();
        let mut input = CommandDefinition::from_signature("components:cancel")
            .unwrap()
            .parse(&[])
            .unwrap();
        let (reader, mut writer) = tokio::io::duplex(1024);
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            reader,
        ))));
        let mut output = Output::default();
        let mut style = OutputStyle::new(&input, &mut output);
        let previous: Arc<dyn QuestionHelperBehavior> = Arc::new(Previous);
        style.set_question_helper(Some(previous.clone()));
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                invoke(&mut style, case)
            )
            .await
            .is_err(),
            "{kind}"
        );
        assert!(
            Arc::ptr_eq(&previous, &style.get_question_helper().unwrap()),
            "{kind}"
        );
        assert!(input.is_interactive());
        writer.write_all(b"after\n").await.unwrap();
        assert_eq!(style.ask("Next").await.unwrap().as_deref(), Some("after"));
    }
}
