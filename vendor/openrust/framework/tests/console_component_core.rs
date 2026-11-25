use openrust::{
    console::{
        CommandDefinition, ConsoleError, ConsoleInputStream, Output, OutputStyle, Question,
        QuestionHelperBehavior,
        view::components::{
            Component, DefaultMutatorConstructor, MutatorConstructor,
            mutators::{
                EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsurePunctuation,
                EnsureRelativePaths, Mutator,
            },
        },
    },
    support::facades::{App, Facade as _},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct SpyConstructor(Arc<AtomicUsize>);
struct Spy(AtomicUsize);
impl MutatorConstructor for SpyConstructor {
    fn make<'a>(&'a self) -> Result<Box<dyn Mutator + 'a>, ConsoleError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Spy(AtomicUsize::new(0))))
    }
}
impl Mutator for Spy {
    fn apply(&self, value: &str) -> Result<String, ConsoleError> {
        Ok(format!(
            "{}:{value}",
            self.0.fetch_add(1, Ordering::SeqCst) + 1
        ))
    }
}
struct Fail;
impl MutatorConstructor for Fail {
    fn make<'a>(&'a self) -> Result<Box<dyn Mutator + 'a>, ConsoleError> {
        Err(ConsoleError::new("构造失败"))
    }
}
fn paths(value: Value, from: &str, to: &str) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace(from, to)),
        Value::Array(v) => Value::Array(v.into_iter().map(|v| paths(v, from, to)).collect()),
        Value::Object(v) => Value::Object(
            v.into_iter()
                .map(|(k, v)| (k, paths(v, from, to)))
                .collect(),
        ),
        v => v,
    }
}
#[tokio::test]
async fn owned_component_mutate_matches_source_data_types_keys_order_and_constructor_lifecycle() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/component-core-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .create()
        .unwrap();
    for has_base in [false, true] {
        App::set_facade_application(if has_base { Some(&app) } else { None }, async {
            for case in cases.iter().filter(|c| c["has_base"] == has_base) {
                let created = Arc::new(AtomicUsize::new(0));
                let spy = SpyConstructor(created.clone());
                let fail = Fail;
                let highlight =
                    DefaultMutatorConstructor::<EnsureDynamicContentIsHighlighted>::new();
                let punctuation = DefaultMutatorConstructor::<EnsurePunctuation>::new();
                let no_punctuation = DefaultMutatorConstructor::<EnsureNoPunctuation>::new();
                let relative = DefaultMutatorConstructor::<EnsureRelativePaths>::new();
                let constructors: Vec<&dyn MutatorConstructor> = case["chain"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| -> &dyn MutatorConstructor {
                        match name.as_str().unwrap() {
                            "EnsureDynamicContentIsHighlighted" => &highlight,
                            "EnsurePunctuation" => &punctuation,
                            "EnsureNoPunctuation" => &no_punctuation,
                            "EnsureRelativePaths" => &relative,
                            "Spy" => &spy,
                            "Fail" => &fail,
                            _ => unreachable!(),
                        }
                    })
                    .collect();
                let mut output = Output::default();
                let component = Component::new(&mut output);
                let data = paths(
                    case["data"].clone(),
                    "__BASE__",
                    root.path().to_str().unwrap(),
                );
                let result = component.mutate(data, &constructors);
                assert_eq!(
                    result.is_err(),
                    !case["error"].is_null(),
                    "{case}: {result:?}"
                );
                if let Ok(value) = result {
                    assert_eq!(
                        paths(value, root.path().to_str().unwrap(), "__BASE__"),
                        case["answer"],
                        "{case}"
                    );
                }
                assert_eq!(
                    created.load(Ordering::SeqCst),
                    case["constructed"].as_u64().unwrap() as usize,
                    "{case}"
                );
                assert_eq!(output.stdout, case["output"], "{case}");
            }
        })
        .await;
    }
}
struct Original;
impl QuestionHelperBehavior for Original {
    fn get_name(&self) -> &str {
        "original"
    }
}
#[tokio::test]
async fn owned_question_scope_accepts_lending_async_closures_and_restores_identity() {
    let mut input = CommandDefinition::from_signature("component:scope")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        std::io::Cursor::new(b"inside\noutside\n".to_vec()),
    ))));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let original: Arc<dyn QuestionHelperBehavior> = Arc::new(Original);
    style.set_question_helper(Some(original.clone()));
    let mut component = Component::new(&mut style);
    let mut calls = 0;
    let answer = component
        .using_question_helper(async |mut style| {
            calls += 1;
            assert!(!Arc::ptr_eq(
                &original,
                &style.get_question_helper().unwrap()
            ));
            let q = Question::new("Inside", None)?;
            style.ask_question(&q).await
        })
        .await
        .unwrap();
    assert_eq!(answer, json!("inside"));
    assert_eq!(calls, 1);
    let error = component
        .using_question_helper(async |_| -> Result<Value, ConsoleError> {
            Err(ConsoleError::new("callback failed"))
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("callback failed"));
    drop(component);
    assert!(Arc::ptr_eq(
        &original,
        &style.get_question_helper().unwrap()
    ));
    assert_eq!(
        style.ask("Outside").await.unwrap(),
        Some("outside".to_owned())
    );
}
#[tokio::test]
async fn owned_question_scope_cancellation_restores_original_and_releases_input() {
    use tokio::io::AsyncWriteExt as _;
    let mut input = CommandDefinition::from_signature("component:cancel")
        .unwrap()
        .parse(&[])
        .unwrap();
    let (reader, mut writer) = tokio::io::duplex(128);
    input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
        reader,
    ))));
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let original: Arc<dyn QuestionHelperBehavior> = Arc::new(Original);
    style.set_question_helper(Some(original.clone()));
    let mut component = Component::new(&mut style);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            component.using_question_helper(async |mut style| {
                let q = Question::new("Pending", None)?;
                style.ask_question(&q).await
            })
        )
        .await
        .is_err()
    );
    writer.write_all(b"recovered\n").await.unwrap();
    assert_eq!(
        component
            .using_question_helper(async |mut style| {
                let q = Question::new("Again", None)?;
                style.ask_question(&q).await
            })
            .await
            .unwrap(),
        json!("recovered")
    );
    drop(component);
    assert!(Arc::ptr_eq(
        &original,
        &style.get_question_helper().unwrap()
    ));
}
#[tokio::test]
async fn owned_question_scope_fails_before_calling_callback_on_plain_output() {
    let mut output = Output::default();
    let mut component = Component::new(&mut output);
    let mut called = false;
    let error = component
        .using_question_helper(async |_| {
            called = true;
            Ok(())
        })
        .await
        .unwrap_err();
    assert!(error.is_runtime());
    assert!(!called);
    assert!(output.stdout.is_empty());
}
#[test]
fn owned_question_scope_restores_helper_when_async_callback_panics() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let input = CommandDefinition::from_signature("component:panic")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    let original: Arc<dyn QuestionHelperBehavior> = Arc::new(Original);
    style.set_question_helper(Some(original.clone()));
    let mut component = Component::new(&mut style);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rt.block_on(
            component.using_question_helper(async |_| -> Result<(), ConsoleError> {
                panic!("callback panic")
            }),
        )
    }));
    assert!(result.is_err());
    assert!(Arc::ptr_eq(
        &original,
        &component
            .question_style()
            .unwrap()
            .get_question_helper()
            .unwrap()
    ));
}
struct CoreConstructor;
struct CoreRenderer<'a> {
    base: Component<'a>,
    original: Arc<dyn QuestionHelperBehavior>,
}
impl openrust::console::view::components::ComponentConstructor for CoreConstructor {
    fn make<'a>(
        &'a self,
        mut output: openrust::console::view::components::ComponentContext<'a>,
    ) -> Result<Box<dyn openrust::console::view::components::RenderableComponent + 'a>, ConsoleError>
    {
        let original: Arc<dyn QuestionHelperBehavior> = Arc::new(Original);
        output
            .question_style()?
            .set_question_helper(Some(original.clone()));
        Ok(Box::new(CoreRenderer {
            base: Component::from_context(output),
            original,
        }))
    }
}
impl openrust::console::view::components::RenderableComponent for CoreRenderer<'_> {
    fn render<'a>(
        &'a mut self,
        parameters: Vec<openrust::console::view::components::ComponentArgument<'a>>,
    ) -> openrust::console::ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            use openrust::console::view::components::ComponentArgument;
            let fail = matches!(
                parameters.first(),
                Some(ComponentArgument::Value(Value::Bool(true)))
            );
            let data = self.base.mutate(
                json!({"title":"Created [record].","flag":true}),
                &[&DefaultMutatorConstructor::<EnsureNoPunctuation>::new()],
            )?;
            assert_eq!(data["flag"], true);
            let value = self
                .base
                .using_question_helper(async |mut style| {
                    if fail {
                        return Err(ConsoleError::new("作用域回调失败"));
                    }
                    let q = Question::new("Name", Some(json!("Guest")))?;
                    style.ask_question(&q).await
                })
                .await;
            assert!(Arc::ptr_eq(
                &self.original,
                &self.base.question_style()?.get_question_helper().unwrap()
            ));
            self.base
                .get_output()
                .write_raw("RESTORED\n")
                .map_err(ConsoleError::from_source)?;
            value
        })
    }
}
struct CoreProvider;
impl openrust::support::ServiceProvider for CoreProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = openrust::support::ProviderResult> + Send + 'a>,
    > {
        Box::pin(async move {
            use openrust::contracts::{container::Container as _, foundation::Application as _};
            app.container()
                .make::<openrust::console::view::components::ComponentRegistry>()?
                .register("Core", Arc::new(CoreConstructor))?;
            Ok(())
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "component:core {--fail} {--answer=}",
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
                let mut io = input.io(output);
                let fail = io.option("fail").unwrap_or(Value::Bool(false));
                let value = io.components().call("Core", vec![fail.into()]).await?;
                io.line(&format!("CORE_VALUE:{value}"))?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由实际CLI/PTY父消费者指定输入和错误模式"]
fn core_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_providers(vec![Box::new(CoreProvider)])
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let mut args: Vec<std::ffi::OsString> = vec!["component:core".into(), "--no-ansi".into()];
    if std::env::var_os("CORE_FAIL").is_some() {
        args.push("--fail".into());
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_uses_owned_component_and_restores_helper_before_success_or_failure() {
    use std::io::Write as _;
    for fail in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "core_cli_driver", "--ignored", "--nocapture"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if fail {
            command.env("CORE_FAIL", "1");
        }
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"reader\n").unwrap();
        let result = child.wait_with_output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(if fail { 1 } else { 0 }),
            "{result:?}"
        );
        let text =
            String::from_utf8(result.stdout).unwrap() + &String::from_utf8(result.stderr).unwrap();
        assert!(text.contains("RESTORED"), "{text}");
        if !fail {
            assert!(text.contains("CORE_VALUE:\"reader\""), "{text}");
        } else {
            assert!(text.contains("作用域回调失败"), "{text}");
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/core", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let label = request
            .query(Some("label"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Guest".into());
        let mut args = vec![format!("--answer={label}\n").into()];
        if request.query(Some("fail"))?.is_some() {
            args.push("--fail".into());
        }
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("component:core", &args, &mut output).await;
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
async fn actual_http_uses_owning_component_and_recovers_after_callback_failure() {
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
        .with_providers(vec![Box::new(CoreProvider)])
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
    let failed = reqwest::get(format!("http://{address}/core?fail=1"))
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    let body: Value = failed.json().await.unwrap();
    assert!(body["output"].as_str().unwrap().contains("RESTORED"));
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..12 {
        requests.spawn(async move {
            let label = format!("reader{i}");
            let response = reqwest::get(format!("http://{address}/core?label={label}"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("CORE_VALUE:\"{label}\"")),
                "{body}"
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
async fn owned_question_scope_matches_fixed_source_prompt_and_finally_lifecycle() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/component-scope-oracle.json"
    ))
    .unwrap();
    for case in &cases {
        let mut input = CommandDefinition::from_signature("component:scope-source")
            .unwrap()
            .parse(&[])
            .unwrap();
        input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(
            std::io::Cursor::new(b"reader\nnext\n".to_vec()),
        ))));
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let mut style = OutputStyle::new(&input, &mut output);
        let original: Option<Arc<dyn QuestionHelperBehavior>> = case["original"]
            .as_bool()
            .unwrap()
            .then(|| Arc::new(Original) as Arc<dyn QuestionHelperBehavior>);
        style.set_question_helper(original.clone());
        let mut component = Component::new(&mut style);
        let mut inside_name = String::new();
        let mut inside_same = false;
        let failure = case["failure"].as_str().unwrap();
        let result = component
            .using_question_helper(async |mut style| {
                let helper = style.get_question_helper().unwrap();
                inside_name = helper.get_name().to_owned();
                inside_same = original
                    .as_ref()
                    .is_some_and(|old| Arc::ptr_eq(old, &helper));
                if failure == "before" {
                    return Err(ConsoleError::new("before"));
                }
                let question = Question::new("Inside", Some(json!("Guest")))?;
                let value = style.ask_question(&question).await?;
                if failure == "after" {
                    return Err(ConsoleError::new("after"));
                }
                Ok(value)
            })
            .await;
        assert_eq!(inside_name, case["inside_name"], "{case}");
        assert_eq!(inside_same, case["inside_same"], "{case}");
        assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
        if let Ok(value) = result {
            assert_eq!(value, case["answer"], "{case}");
        }
        let restored = component
            .question_style()
            .unwrap()
            .get_question_helper()
            .unwrap();
        assert_eq!(restored.get_name(), case["after_name"], "{case}");
        if let Some(old) = original.as_ref() {
            assert!(Arc::ptr_eq(old, &restored));
        }
        drop(component);
        let q = Question::new("Outside", Some(json!("Guest"))).unwrap();
        let outside = openrust::console::QuestionStyle::ask_question(&mut style, &q)
            .await
            .unwrap();
        assert_eq!(outside, case["outside"], "{case}");
        assert_eq!(
            style.new_lines_written(),
            case["new_lines"].as_u64().unwrap() as usize,
            "{case}"
        );
        assert_eq!(output.stdout, case["output"], "{case}");
        assert!(output.stderr.is_empty());
    }
}
