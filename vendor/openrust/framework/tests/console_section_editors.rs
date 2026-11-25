use openrust::{
    console::{AsyncCommandCallback, ConsoleError, ConsoleInputStream, OutputOptions, Question},
    foundation::{Application, configuration::Routing},
    support::facades::Artisan,
};
use serde_json::Value;
fn register_case(case: Value) -> Result<(), ConsoleError> {
    Artisan::command_with(
        "sections:editor {--invalid}",
        AsyncCommandCallback::new(move |_, input, output| {
            let case = case.clone();
            Box::pin(async move {
                let mut reading = input.clone();
                if let Some(answer) = case["answer"].as_str() {
                    reading.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
                        std::io::Cursor::new(answer.as_bytes().to_vec()),
                    ))));
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
                let kind = case["kind"].as_str().unwrap();
                let mut question = if kind == "choice" || kind == "choice-many" {
                    let mut choice = openrust::console::ChoiceQuestion::new(
                        "Name",
                        case["values"].as_array().unwrap().clone(),
                        case.get("default").filter(|v| !v.is_null()).cloned(),
                    )?;
                    choice.set_multiselect(kind == "choice-many");
                    choice.set_max_attempts(Some(1))?;
                    choice.into()
                } else {
                    Question::new(
                        "Name",
                        case.get("default").filter(|v| !v.is_null()).cloned(),
                    )?
                };
                if input.option("invalid").and_then(|v| v.as_bool()) == Some(true) {
                    question.set_hidden(true)?;
                    question.set_autocompleter_values(Some(vec!["Ada".into()]))?;
                }
                let kind = case["kind"].as_str().unwrap();
                question.set_multiline(kind.contains("multi"));
                if kind.contains("complete") {
                    question.set_autocompleter_values(Some(
                        case["values"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap().to_owned())
                            .collect(),
                    ))?;
                }
                let mut section = output.section_output(&first)?;
                let value = {
                    let mut style = section.style(&reading);
                    style.ask_question(&question).await?
                };
                let before = section.get_content();
                section.clear(None)?;
                section.writeln("after", OutputOptions::default())?;
                let meta = serde_json::json!({"value":value,"before":before,"after":section.get_content()});
                output
                    .stderr
                    .push_str(&format!("SECTION_EDITOR_META:{meta}\n"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn register() -> Result<(), ConsoleError> {
    register_case(serde_json::from_str(&std::env::var("SECTION_EDITOR_CASE").unwrap()).unwrap())
}
#[test]
#[ignore = "由真实终端编辑器验收脚本驱动"]
fn section_editor_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::env::var_os("SECTION_EDITOR_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let case: Value = serde_json::from_str(&std::env::var("SECTION_EDITOR_CASE").unwrap()).unwrap();
    let args = [
        "sections:editor".into(),
        if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        }
        .into(),
    ];
    let status = runtime.block_on(app.handle_command(&args)).unwrap();
    if let Some(release) = std::env::var_os("SECTION_EDITOR_PROBE_RELEASE") {
        eprintln!("SECTION_EDITOR_STATUS:{status}");
        let release = std::path::PathBuf::from(release);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !release.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    std::process::exit(status);
}
fn multi_case() -> Value {
    serde_json::json!({"kind":"multi","default":"Guest","values":["Ada","Adam","Beta"],"decorated":false})
}
fn expected_multi() -> Value {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/section-editor-oracle.json"
    ))
    .unwrap();
    cases
        .iter()
        .find(|case| {
            case["kind"] == "multi"
                && case["keys"] == "one\rtwo\r\u{4}"
                && case["decorated"] == false
        })
        .unwrap()["metadata"]
        .clone()
}
#[test]
fn real_artisan_multiline_pipe_preserves_selected_section_and_answer() {
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "section_editor_cli_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("SECTION_EDITOR_ROOT", root.path())
        .env("SECTION_EDITOR_CASE", multi_case().to_string())
        .env("COLUMNS", "80")
        .env("APP_LOCALE", "en")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"one\ntwo\n\x04")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stderr).unwrap();
    let actual: Value =
        serde_json::from_str(text.trim().strip_prefix("SECTION_EDITOR_META:").unwrap()).unwrap();
    assert_eq!(actual, expected_multi());
}
fn register_http() -> Result<(), ConsoleError> {
    let mut case = multi_case();
    case["answer"] = serde_json::json!("one\ntwo\n\u{4}");
    register_case(case)
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/section-editor", false), ("/section-editor/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = openrust::console::Output::default();
            let mut args = vec!["--no-ansi".into()];
            if invalid {
                args.push("--invalid".into());
            }
            Artisan::call("sections:editor", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            let meta: Value = serde_json::from_str(
                output
                    .stderr
                    .trim()
                    .strip_prefix("SECTION_EDITOR_META:")
                    .unwrap(),
            )
            .unwrap();
            openrust::http::Response::json(http::StatusCode::OK, meta)
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_multiline_default_json_failure_then_recovery() {
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
        .with_routing(Routing::default().commands(register_http).web(routes))
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
        ("/section-editor", 200),
        ("/section-editor/error", 500),
        ("/section-editor", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(
            response.headers()[reqwest::header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let body: Value = response.json().await.unwrap();
        if status == 200 {
            assert_eq!(body, expected_multi());
        } else {
            assert!(body.is_object());
        }
    }
    server.abort();
    let _ = server.await;
}
