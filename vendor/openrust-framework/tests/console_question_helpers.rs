use openrust::console::{
    AsOutput, ChoiceQuestion, CommandDefinition, ConfirmationQuestion, ConsoleError,
    ConsoleInputStream, ConsoleStyle, DefaultQuestionHelper, Input, Output, OutputOptions,
    OutputStyle, Question, QuestionCallback, QuestionHelper, QuestionHelperBehavior, QuestionKind,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn question(case: &Value) -> Question {
    let default = case.get("default").filter(|v| !v.is_null()).cloned();
    let mut question = match case["kind"].as_str().unwrap() {
        "confirmation" => ConfirmationQuestion::new(
            case["label"].as_str().unwrap(),
            case["default"].as_bool().unwrap(),
            "/^y/i",
        )
        .unwrap()
        .into(),
        "choice" => {
            let mut q = ChoiceQuestion::new(
                case["label"].as_str().unwrap(),
                ["First", "Second"],
                default,
            )
            .unwrap();
            q.set_multiselect(case["multiple"].as_bool().unwrap());
            q.into()
        }
        _ => Question::new(case["label"].as_str().unwrap(), default).unwrap(),
    };
    if case["retry"] == true {
        question.set_validator(Some(QuestionCallback::sync(|value| {
            if value == "bad" {
                Err(ConsoleError::new("Try again"))
            } else {
                Ok(json!(value.as_str().unwrap().to_uppercase()))
            }
        })));
    }
    question
}
async fn ask(
    case: &Value,
    input: &Input,
    output: &mut (impl AsOutput + ?Sized),
    question: &Question,
) -> Result<Value, ConsoleError> {
    if case["helper"] == "component" {
        QuestionHelper::new().ask(input, output, question).await
    } else {
        DefaultQuestionHelper::new()
            .ask(input, output, question)
            .await
    }
}
async fn check_case(case: &Value) {
    let mut input = CommandDefinition::from_signature("helpers:ask")
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
    let question = question(case);
    let (result, count) = match case["surface"].as_str().unwrap() {
        "raw" => (ask(case, &input, &mut output, &question).await, Value::Null),
        "base" => {
            let mut style = ConsoleStyle::new(&input, &mut output);
            (ask(case, &input, &mut style, &question).await, Value::Null)
        }
        "nested" => {
            let mut parent = OutputStyle::new(&input, &mut output);
            let mut style = OutputStyle::new(&input, &mut parent);
            let result = ask(case, &input, &mut style, &question).await;
            (result, json!(style.new_lines_written()))
        }
        _ => {
            let mut style = OutputStyle::new(&input, &mut output);
            let result = ask(case, &input, &mut style, &question).await;
            (result, json!(style.new_lines_written()))
        }
    };
    assert_eq!(
        result.is_err(),
        !case["error"].is_null(),
        "{case}: {result:?}"
    );
    if let Ok(answer) = result {
        assert_eq!(answer, case["answer"], "{case}");
    }
    assert_eq!(output.stdout, case["output"], "{case}");
    assert!(output.stderr.is_empty());
    assert_eq!(count, case["new_lines"], "{case}");
    assert_eq!(input.is_interactive(), case["interactive_after"], "{case}");
}
#[test]
#[ignore = "由父消费者设置真实COLUMNS并执行完整帮助器矩阵"]
fn helper_matrix_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/question-helper-oracle.json"
    ))
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        for case in &cases {
            check_case(case).await;
        }
    });
    println!("HELPER_CASES_OK:{}", cases.len());
}
#[test]
fn both_formal_helper_types_match_actual_source_on_all_builtin_surfaces() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "helper_matrix_driver",
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
    assert!(String::from_utf8_lossy(&result.stdout).contains("HELPER_CASES_OK:68"));
}
struct CustomHelper(Arc<AtomicUsize>);
impl QuestionHelperBehavior for CustomHelper {
    fn get_name(&self) -> &str {
        "custom"
    }
    fn write_prompt(
        &self,
        output: &mut OutputStyle<'_>,
        question: &Question,
    ) -> Result<String, ConsoleError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        output.write(
            format!("custom:{} > ", question.get_question()).as_str(),
            false,
            OutputOptions::default(),
        )?;
        Ok(format!("custom:{} > ", question.get_question()))
    }
}
#[tokio::test]
async fn temporary_helper_preserves_original_identity_after_success_error_and_cancellation() {
    let mut input = CommandDefinition::from_signature("helpers:scope")
        .unwrap()
        .parse(&[])
        .unwrap();
    let (reader, mut writer) = tokio::io::duplex(4096);
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        reader,
    ))));
    use tokio::io::AsyncWriteExt as _;
    writer.write_all(b"first\nsecond\nthird\n").await.unwrap();
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    assert!(style.get_question_helper().is_none());
    let calls = Arc::new(AtomicUsize::new(0));
    let original: Arc<dyn QuestionHelperBehavior> = Arc::new(CustomHelper(calls.clone()));
    style.set_question_helper(Some(original.clone()));
    let question = Question::new("Name", None).unwrap();
    {
        let mut scope = style.using_question_helper(Arc::new(QuestionHelper::new()));
        assert_eq!(scope.ask_question(&question).await.unwrap(), "first");
    }
    assert!(Arc::ptr_eq(
        &style.get_question_helper().unwrap(),
        &original
    ));
    assert_eq!(style.ask_question(&question).await.unwrap(), "second");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut invalid = question.clone();
    invalid
        .set_max_attempts(Some(1))
        .unwrap()
        .set_validator(Some(QuestionCallback::sync(|_| {
            Err(ConsoleError::new("validation failed"))
        })));
    {
        let mut scope = style.using_question_helper(Arc::new(QuestionHelper::new()));
        assert_eq!(
            scope.ask_question(&invalid).await.unwrap_err().to_string(),
            "validation failed"
        );
    }
    assert!(Arc::ptr_eq(
        &style.get_question_helper().unwrap(),
        &original
    ));
    let future = async {
        let mut scope = style.using_question_helper(Arc::new(QuestionHelper::new()));
        scope.ask_question(&question).await
    };
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), future)
            .await
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &style.get_question_helper().unwrap(),
        &original
    ));
    writer.write_all(b"fourth\n").await.unwrap();
    assert_eq!(style.ask_question(&question).await.unwrap(), "fourth");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn uninitialized_scope_restores_default_and_metadata_views_borrow_original_choices() {
    let mut input = CommandDefinition::from_signature("helpers:default")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_interactive(false);
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    assert!(style.get_question_helper().is_none());
    {
        let mut scope = style.using_question_helper(Arc::new(QuestionHelper::new()));
        assert_eq!(
            scope
                .ask_with("Name", Some("Guest"))
                .await
                .unwrap()
                .as_deref(),
            Some("Guest")
        );
    }
    assert_eq!(style.get_question_helper().unwrap().get_name(), "question");
    let plain = Question::new("Text", None).unwrap();
    assert!(matches!(plain.get_kind(), QuestionKind::Text));
    let confirmation = ConfirmationQuestion::new("Proceed", false, "/^y/i").unwrap();
    assert!(matches!(
        confirmation.get_kind(),
        QuestionKind::Confirmation
    ));
    let choice = ChoiceQuestion::new("Color", ["red", "green"], None).unwrap();
    match choice.get_kind() {
        QuestionKind::Choice {
            choices,
            multiselect,
            prompt,
        } => {
            assert!(std::ptr::eq(choices, choice.get_choices()));
            assert!(!multiselect);
            assert_eq!(prompt, choice.get_prompt());
        }
        _ => panic!("缺少选择种类元数据"),
    }
    assert_eq!(QuestionHelper::new().get_name(), "question");
    assert_eq!(DefaultQuestionHelper::new().get_name(), "question");
    for (text, expected) in [
        ("Name", "Name:"),
        ("Name?", "Name?"),
        ("Name.", "Name."),
        ("Name!", "Name!"),
        ("Name:", "Name:"),
        ("", ":"),
    ] {
        assert_eq!(
            QuestionHelper::new().ensure_ends_with_punctuation(text),
            expected
        );
    }
    QuestionHelper::disable_stty();
    DefaultQuestionHelper::disable_stty();
}
#[tokio::test]
async fn scope_contract_also_accepts_base_styles_and_dynamic_styles() {
    use openrust::console::{QuestionHelperScope, QuestionStyle};
    async fn invoke(style: &mut dyn QuestionStyle, question: &Question) {
        let previous = style.get_question_helper();
        {
            let mut scope = QuestionHelperScope::new(style, Arc::new(QuestionHelper::new()));
            assert_eq!(scope.ask_question(question).await.unwrap(), "Guest");
        }
        if let Some(previous) = previous {
            assert!(Arc::ptr_eq(
                &previous,
                &style.get_question_helper().unwrap()
            ));
        } else {
            assert_eq!(style.get_question_helper().unwrap().get_name(), "question");
        }
    }
    let mut input = CommandDefinition::from_signature("helpers:styles")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_interactive(false);
    let question = Question::new("Name", Some(json!("Guest"))).unwrap();
    let mut output = Output::default();
    invoke(&mut OutputStyle::new(&input, &mut output), &question).await;
    let mut style = ConsoleStyle::new(&input, &mut output);
    let calls = Arc::new(AtomicUsize::new(0));
    let original: Arc<dyn QuestionHelperBehavior> = Arc::new(CustomHelper(calls));
    style.set_question_helper(Some(original.clone()));
    invoke(&mut style, &question).await;
    {
        let mut guard = style.using_question_helper(Arc::new(QuestionHelper::new()));
        assert_eq!(guard.ask_question(&question).await.unwrap(), "Guest");
    }
    assert!(Arc::ptr_eq(
        &original,
        &style.get_question_helper().unwrap()
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut guard = style.using_question_helper(Arc::new(QuestionHelper::new()));
        guard.set_question_helper(None);
        panic!("sentinel");
    }));
    assert!(result.is_err());
    assert!(Arc::ptr_eq(
        &original,
        &style.get_question_helper().unwrap()
    ));
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "helpers:show {--answer=} {--invalid}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut input = input.clone();
                if let Some(answer) = input
                    .option("answer")
                    .and_then(|v| v.as_str().map(str::to_owned))
                {
                    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(answer.into_bytes()),
                    ))));
                }
                let case = std::env::var("HELPER_CASE")
                    .ok()
                    .map(|v| serde_json::from_str::<Value>(&v).unwrap())
                    .unwrap_or_else(|| json!({"helper":"component","surface":"style"}));
                let mut question = Question::new("Name", Some(json!("Guest")))?;
                if input.option("invalid").and_then(|v| v.as_bool()) == Some(true) {
                    question.set_max_attempts(Some(1))?.set_validator(Some(
                        QuestionCallback::sync(|_| Err(ConsoleError::new("validation failed"))),
                    ));
                }
                let value = if case["surface"] == "raw" {
                    ask(&case, &input, output, &question).await?
                } else {
                    ask(
                        &case,
                        &input,
                        &mut OutputStyle::new(&input, output),
                        &question,
                    )
                    .await?
                };
                output.as_output().writeln(
                    format!("HELPER_VALUE:{}", serde_json::to_string(&value).unwrap()).as_str(),
                    OutputOptions::default(),
                )?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行实际Artisan及stdin/stdout/stderr"]
fn helper_cli_driver() {
    let case: Value = serde_json::from_str(&std::env::var("HELPER_CASE").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut args: Vec<std::ffi::OsString> = vec![
        "helpers:show".into(),
        if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        }
        .into(),
    ];
    if case["invalid"] == true {
        args.push("--invalid".into());
    }
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_console_streams_match_source_and_errors_exit_one() {
    use std::io::Write as _;
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/question-helper-console-oracle.json"
    ))
    .unwrap();
    for case in cases {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "helper_cli_driver", "--ignored", "--nocapture"])
            .env("COLUMNS", "80")
            .env("HELPER_CASE", case.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"answer\n").unwrap();
        let result = child.wait_with_output().unwrap();
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
    let case = json!({"helper":"component","surface":"style","decorated":false,"invalid":true});
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "helper_cli_driver", "--ignored", "--nocapture"])
        .env("HELPER_CASE", case.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"bad\n").unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("validation failed")
            || String::from_utf8_lossy(&result.stderr).contains("validation failed")
    );
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/helpers", helper_response)?;
    Ok(())
}
fn helper_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let answer = request
            .query(Some("answer"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Guest".into());
        let invalid = request.query(Some("invalid"))?.is_some();
        let mut args = vec![format!("--answer={answer}\n").into()];
        if invalid {
            args.push("--invalid".into());
        }
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("helpers:show", &args, &mut output).await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"output":output.stdout,"error":output.stderr}),
        )
    })
}
#[tokio::test]
async fn actual_http_command_calls_restore_after_errors_and_parallel_requests() {
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
    for (query, status, answer) in [
        ("?answer=First", 200, Some("First")),
        ("?answer=bad&invalid=1", 500, None),
        ("?answer=Recovered", 200, Some("Recovered")),
    ] {
        let response = reqwest::get(format!("http://{address}/helpers{query}"))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body: Value = response.json().await.unwrap();
        if let Some(answer) = answer {
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("HELPER_VALUE:\"{answer}\"")),
                "{body}"
            );
        }
    }
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..12 {
        requests.spawn(async move {
            let answer = format!("Guest{i}");
            let response = reqwest::get(format!("http://{address}/helpers?answer={answer}"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("HELPER_VALUE:\"{answer}\""))
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
async fn restored_default_is_used_for_the_following_question_not_the_component_prompt() {
    let mut input = CommandDefinition::from_signature("helpers:restore")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"first\nsecond\n".to_vec()),
    ))));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    {
        let mut scope = style.using_question_helper(Arc::new(QuestionHelper::new()));
        assert_eq!(scope.ask("First").await.unwrap().as_deref(), Some("first"));
    }
    assert_eq!(
        style.ask("Second").await.unwrap().as_deref(),
        Some("second")
    );
    assert!(output.stdout.contains("  First:\n❯ "), "{}", output.stdout);
    assert!(output.stdout.contains(" Second:\n > "), "{}", output.stdout);
    assert!(!output.stdout.contains("  Second:"));
}
