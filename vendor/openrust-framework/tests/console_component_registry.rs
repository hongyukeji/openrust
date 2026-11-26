use openrust::{
    console::{
        AsOutput as _, CommandDefinition, ConsoleError, ConsoleFuture, Output, OutputFormat,
        OutputStyle,
        view::components::{
            ComponentArgument, ComponentConstructor, ComponentContext, ComponentRegistry, Factory,
            RenderableComponent,
        },
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Facade as _},
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct BadgeConstructor {
    owner: &'static str,
    created: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}
struct Badge<'a> {
    output: ComponentContext<'a>,
    owner: &'static str,
    index: usize,
    dropped: Arc<AtomicUsize>,
}
impl Drop for Badge<'_> {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl ComponentConstructor for BadgeConstructor {
    fn make<'a>(
        &'a self,
        output: ComponentContext<'a>,
    ) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Badge {
            output,
            owner: self.owner,
            index: 0,
            dropped: self.dropped.clone(),
        }))
    }
}
impl RenderableComponent for Badge<'_> {
    fn render<'a>(
        &'a mut self,
        mut parameters: Vec<ComponentArgument<'a>>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            let label = match parameters.remove(0) {
                ComponentArgument::Value(Value::String(label)) => label,
                _ => return Err(ConsoleError::new("需要标签")),
            };
            if label == "fail" {
                return Err(ConsoleError::new("组件渲染失败"));
            }
            if label == "pending" {
                std::future::pending::<()>().await;
            }
            self.index += 1;
            self.output.as_output().writeln(
                format!("badge:{}:{label}:{}", self.owner, self.index).as_str(),
                OutputFormat::Raw,
            )?;
            Ok(json!({"owner":self.owner,"label":label,"index":self.index}))
        })
    }
}
fn constructor(owner: &'static str) -> (Arc<BadgeConstructor>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let created = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(BadgeConstructor {
            owner,
            created: created.clone(),
            dropped: dropped.clone(),
        }),
        created,
        dropped,
    )
}
#[tokio::test]
async fn standalone_registry_constructs_fresh_instances_and_drops_on_error_or_cancel() {
    let registry = Arc::new(ComponentRegistry::default());
    let (maker, created, dropped) = constructor("A");
    registry.register("Widgets\\Badge", maker).unwrap();
    let mut output = Output::default();
    let mut factory = Factory::with_registry(&mut output, registry);
    for label in ["one", "two"] {
        assert_eq!(
            factory
                .call("widgets\\BADGE", vec![label.into()])
                .await
                .unwrap(),
            json!({"owner":"A","label":label,"index":1})
        );
    }
    assert!(
        factory
            .call("Widgets\\Badge", vec!["fail".into()])
            .await
            .unwrap_err()
            .to_string()
            .contains("渲染失败")
    );
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            factory.call("Widgets\\Badge", vec!["pending".into()])
        )
        .await
        .is_err()
    );
    assert_eq!(created.load(Ordering::SeqCst), 4);
    assert_eq!(dropped.load(Ordering::SeqCst), 4);
    assert_eq!(output.stdout, "badge:A:one:1\nbadge:A:two:1\n");
}
struct QuestionConstructor;
struct QuestionComponent<'a>(ComponentContext<'a>);
impl ComponentConstructor for QuestionConstructor {
    fn make<'a>(
        &'a self,
        output: ComponentContext<'a>,
    ) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
        Ok(Box::new(QuestionComponent(output)))
    }
}
impl RenderableComponent for QuestionComponent<'_> {
    fn render<'a>(
        &'a mut self,
        _: Vec<ComponentArgument<'a>>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            openrust::console::view::components::Ask::new(self.0.question_style()?)
                .render_with("Name", Some(json!("Guest")), false)
                .await
        })
    }
}
#[tokio::test]
async fn custom_question_uses_original_style_and_plain_output_fails_before_prompt() {
    let registry = Arc::new(ComponentRegistry::default());
    registry
        .register("Question", Arc::new(QuestionConstructor))
        .unwrap();
    let mut input = CommandDefinition::from_signature("custom:question")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_interactive(false);
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    assert_eq!(
        Factory::with_registry(&mut style, registry.clone())
            .call("Question", vec![])
            .await
            .unwrap(),
        json!("Guest")
    );
    let mut output = Output::default();
    assert!(
        Factory::with_registry(&mut output, registry)
            .call("Question", vec![])
            .await
            .unwrap_err()
            .is_runtime()
    );
    assert!(output.stdout.is_empty());
}
#[test]
fn registration_reserves_builtins_and_rejects_invalid_or_duplicate_names() {
    let registry = ComponentRegistry::default();
    for name in [
        "",
        "1Badge",
        "bad-name",
        "\\Badge",
        "Badge\\",
        "Badge\\\\Child",
        "Info",
        "ASKWITHCOMPLETION",
        "Factory",
        "Component",
        "Mutators\\EnsurePunctuation",
    ] {
        assert!(
            registry.register(name, constructor("A").0).is_err(),
            "{name}"
        );
    }
    registry.register("Badge", constructor("A").0).unwrap();
    assert!(registry.register("BADGE", constructor("B").0).is_err());
    registry.register("中文\\徽章", constructor("A").0).unwrap();
}
struct FailConstructor;
impl ComponentConstructor for FailConstructor {
    fn make<'a>(
        &'a self,
        _: ComponentContext<'a>,
    ) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
        Err(ConsoleError::new("构造失败"))
    }
}
#[tokio::test]
async fn constructor_error_keeps_output_empty_and_other_components_recover() {
    let registry = Arc::new(ComponentRegistry::default());
    registry
        .register("Fail", Arc::new(FailConstructor))
        .unwrap();
    registry.register("Badge", constructor("A").0).unwrap();
    let mut output = Output::default();
    let mut factory = Factory::with_registry(&mut output, registry);
    assert!(
        factory
            .call("Fail", vec![])
            .await
            .unwrap_err()
            .to_string()
            .contains("构造失败")
    );
    assert!(factory.get_output().stdout.is_empty());
    factory
        .call("Badge", vec!["recovered".into()])
        .await
        .unwrap();
    assert!(output.stdout.contains("recovered"));
}
struct RegistryProvider(&'static str);
impl ServiceProvider for RegistryProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container()
                .make::<ComponentRegistry>()?
                .register("Badge", constructor(self.0).0)?;
            Ok(())
        })
    }
}
fn application(path: &std::path::Path, owner: &'static str) -> Application {
    Application::configure(path.into())
        .with_providers(vec![Box::new(RegistryProvider(owner))])
        .create()
        .unwrap()
}
#[tokio::test]
async fn application_registration_is_isolated_and_factory_retains_its_owner() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut a = application(roots[0].path(), "A");
    let mut b = application(roots[1].path(), "B");
    a.boot().await.unwrap();
    b.boot().await.unwrap();
    let mut output = Output::default();
    App::set_facade_application(Some(&a), async {
        let mut factory = Factory::new(&mut output);
        App::set_facade_application(Some(&b), async {
            assert_eq!(
                factory.call("Badge", vec!["owned".into()]).await.unwrap()["owner"],
                "A"
            );
        })
        .await;
    })
    .await;
    App::set_facade_application(Some(&b), async {
        assert_eq!(
            Factory::new(&mut output)
                .call("Badge", vec!["other".into()])
                .await
                .unwrap()["owner"],
            "B"
        );
    })
    .await;
    let mut no_app = Output::default();
    assert!(
        Factory::new(&mut no_app)
            .call("Badge", vec!["missing".into()])
            .await
            .is_err()
    );
    assert!(no_app.stdout.is_empty());
}
#[tokio::test]
async fn custom_constructor_and_destructor_match_fixed_source() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/factory-registry-oracle.json"
    ))
    .unwrap();
    let registry = Arc::new(ComponentRegistry::default());
    let (maker, created, dropped) = constructor("A");
    registry.register("Widgets\\Badge", maker).unwrap();
    for case in &cases {
        let mut output = Output::default();
        let result = Factory::with_registry(&mut output, registry.clone())
            .call("Widgets\\Badge", vec![case["label"].clone().into()])
            .await;
        assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
        if let Ok(value) = result {
            assert_eq!(value, case["answer"], "{case}");
        }
        assert_eq!(output.stdout, case["output"], "{case}");
        assert_eq!(
            created.load(Ordering::SeqCst),
            case["created"].as_u64().unwrap() as usize
        );
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            case["dropped"].as_u64().unwrap() as usize
        );
    }
}
#[test]
fn constructor_and_rejected_constructor_drop_do_not_hold_registry_locks() {
    struct Nested {
        registry: std::sync::Weak<ComponentRegistry>,
        badge: Arc<BadgeConstructor>,
    }
    impl ComponentConstructor for Nested {
        fn make<'a>(
            &'a self,
            output: ComponentContext<'a>,
        ) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
            self.registry
                .upgrade()
                .unwrap()
                .register("Late", constructor("A").0)?;
            self.badge.make(output)
        }
    }
    struct Rejected(std::sync::Weak<ComponentRegistry>);
    impl Drop for Rejected {
        fn drop(&mut self) {
            self.0
                .upgrade()
                .unwrap()
                .register("FromDrop", constructor("A").0)
                .unwrap();
        }
    }
    impl ComponentConstructor for Rejected {
        fn make<'a>(
            &'a self,
            _: ComponentContext<'a>,
        ) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
            Err(ConsoleError::new("未使用的构造器"))
        }
    }
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let registry = Arc::new(ComponentRegistry::default());
        registry
            .register(
                "Nested",
                Arc::new(Nested {
                    registry: Arc::downgrade(&registry),
                    badge: constructor("A").0,
                }),
            )
            .unwrap();
        assert!(
            registry
                .register("NESTED", Arc::new(Rejected(Arc::downgrade(&registry))))
                .is_err()
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut output = Output::default();
        rt.block_on(async {
            for name in ["Nested", "Late", "FromDrop"] {
                Factory::with_registry(&mut output, registry.clone())
                    .call(name, vec!["ok".into()])
                    .await
                    .unwrap();
            }
        });
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "registry:badge {label=Guest}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut io = input.io(output);
                let label = io.argument("label").unwrap();
                let result = io.components().call("Badge", vec![label.into()]).await?;
                io.line(&serde_json::to_string(&result).unwrap())?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    let handler: for<'a> fn(
        &'a mut openrust::http::Request,
    ) -> openrust::http::HttpFuture<
        'a,
        Result<openrust::http::Response, openrust::http::HttpError>,
    > = |request| {
        Box::pin(async move {
            let label = request
                .query(Some("label"))?
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "Guest".into());
            let mut output = Output::default();
            let result = openrust::support::facades::Artisan::call(
                "registry:badge",
                &[label.into()],
                &mut output,
            )
            .await;
            openrust::http::Response::json(
                if result.is_ok() {
                    http::StatusCode::OK
                } else {
                    http::StatusCode::INTERNAL_SERVER_ERROR
                },
                json!({"output":output.stdout,"error":output.stderr}),
            )
        })
    };
    openrust::support::facades::Route::get("/registry", handler)?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行正式Artisan入口并检查实际退出码"]
fn registry_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_providers(vec![Box::new(RegistryProvider("CLI"))])
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let label = std::env::var("REGISTRY_LABEL").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt
        .block_on(app.handle_command(&["registry:badge".into(), label.into(), "--no-ansi".into()]))
        .unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_custom_components_return_success_and_failure_exit_codes() {
    for (label, code) in [("one", 0), ("fail", 1), ("recovered", 0)] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "registry_cli_driver", "--ignored", "--nocapture"])
            .env("REGISTRY_LABEL", label)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        let text =
            String::from_utf8(result.stdout).unwrap() + &String::from_utf8(result.stderr).unwrap();
        if code == 0 {
            assert!(text.contains(&format!("badge:CLI:{label}:1")), "{text}");
        } else {
            assert!(text.contains("组件渲染失败"), "{text}");
        }
    }
}
#[tokio::test]
async fn actual_http_two_applications_keep_components_isolated_after_error_and_parallel_calls() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut servers = Vec::new();
    let mut addresses = Vec::new();
    for (index, owner) in ["A", "B"].into_iter().enumerate() {
        let app = Application::configure(roots[index].path().into())
            .with_providers(vec![Box::new(RegistryProvider(owner))])
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
        addresses.push(listener.local_addr().unwrap());
        servers.push(tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap()
        }));
    }
    for address in &addresses {
        let response = reqwest::get(format!("http://{address}/registry?label=fail"))
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
    }
    let mut requests = tokio::task::JoinSet::new();
    for (index, address) in addresses.iter().copied().enumerate() {
        for i in 0..12 {
            requests.spawn(async move {
                let label = format!("r{i}");
                let response = reqwest::get(format!("http://{address}/registry?label={label}"))
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let body: Value = response.json().await.unwrap();
                let owner = if index == 0 { "A" } else { "B" };
                assert!(
                    body["output"]
                        .as_str()
                        .unwrap()
                        .contains(&format!("badge:{owner}:{label}:1")),
                    "{body}"
                );
            });
        }
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    for server in servers {
        server.abort();
        let _ = server.await;
    }
}
#[tokio::test]
async fn custom_output_projection_retains_input_for_builtin_and_registered_questions() {
    struct Adapter<'a>(OutputStyle<'a>);
    impl openrust::console::AsOutput for Adapter<'_> {
        fn as_output(&mut self) -> openrust::console::OutputReference<'_> {
            self.0.as_output()
        }
    }
    impl openrust::console::view::components::ComponentOutput for Adapter<'_> {
        fn component_context(&mut self) -> ComponentContext<'_> {
            ComponentContext::Style(&mut self.0)
        }
    }
    let registry = Arc::new(ComponentRegistry::default());
    registry
        .register("Question", Arc::new(QuestionConstructor))
        .unwrap();
    let mut input = CommandDefinition::from_signature("adapter:question")
        .unwrap()
        .parse(&[])
        .unwrap();
    input.set_interactive(false);
    let mut output = Output::default();
    let mut adapter = Box::new(Adapter(OutputStyle::new(&input, &mut output)));
    let mut factory = Factory::with_registry(&mut adapter, registry);
    assert_eq!(
        factory.call("Question", vec![]).await.unwrap(),
        json!("Guest")
    );
    assert_eq!(
        factory
            .ask_with("Name", Some(json!("Original")), false)
            .await
            .unwrap(),
        json!("Original")
    );
}
#[tokio::test]
async fn registry_binding_errors_are_preserved_and_rebinding_can_recover() {
    use openrust::{container::ServiceKey, contracts::container::BindingResolutionError};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into()).create().unwrap();
    let mut output = Output::default();
    App::set_facade_application(Some(&app), async {
        let mut factory = Factory::new(&mut output);
        app.container()
            .bind::<ComponentRegistry, _>(|_| {
                Err(BindingResolutionError::InvalidConstructor {
                    service: ServiceKey::typed::<ComponentRegistry>(),
                    reason: "注册表不可用",
                })
            })
            .unwrap();
        let error = factory
            .call("Badge", vec!["blocked".into()])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("注册表不可用"));
        assert!(factory.get_output().stdout.is_empty());
        let registry = Arc::new(ComponentRegistry::default());
        registry
            .register("Badge", constructor("Recovered").0)
            .unwrap();
        app.container().instance(registry).unwrap();
        assert_eq!(
            factory.call("Badge", vec!["ok".into()]).await.unwrap()["owner"],
            "Recovered"
        );
    })
    .await;
}
