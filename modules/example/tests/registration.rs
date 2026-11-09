use super::config_sources::enabled_example_sources;
use super::services::{
    EXAMPLE_PRESENTER_ALIAS, ExampleMessage, ExamplePresenter, ExampleServedAt, GreetingPresenter,
};
use openrust::container::{
    AttributeValue, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey,
};
use openrust::http::{
    HttpServiceProvider,
    axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    },
};
use openrust_modules::ModuleProviderBuilderExt as _;
use openrust_modules::ModuleTestAppBuilderExt as _;
use tower::ServiceExt;

// 生产日志服务仍由标准Provider构造，模块路由继续使用同一启动管线。
#[tokio::test]
async fn production_log_services_coexist_with_normal_module_routes()
-> Result<(), Box<dyn std::error::Error>> {
    let source = enabled_example_sources(&production_sources(true))?.merge_ordered_entries([
        ("app", serde_json::json!({"env":"production"})),
        ("mail", serde_json::json!({"driver":"log"})),
        ("sms", serde_json::json!({"driver":"log"})),
    ])?;
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    let outcome: Result<(), Box<dyn std::error::Error>> = async {
        if !app.app()?.is_production() {
            return Err("生产配置未进入实际App".into());
        }
        let _ = app
            .app()?
            .state()
            .resolve::<openrust::mail::Mailer>()
            .await?;
        let _ = app
            .app()?
            .state()
            .resolve::<openrust::sms::SmsSender>()
            .await?;
        let state = app.app()?.state();
        let alias = ServiceKey::named(EXAMPLE_PRESENTER_ALIAS);
        assert!(state.container().contains_key(&alias));
        assert_eq!(
            state.container().get_alias(&alias)?,
            ServiceKey::typed::<ExamplePresenter>()
        );
        assert!(!state.container().contains::<ExamplePresenter>());
        let abstract_key = ServiceKey::typed::<dyn GreetingPresenter>();
        assert!(state.container().contains_key(&abstract_key));
        assert!(!state.container().resolved_key(&abstract_key)?);
        assert!(!state.container().resolved_key(&alias)?);
        let message_binding = state
            .container()
            .get_bindings()?
            .into_iter()
            .find(|binding| binding.key == ServiceKey::typed::<ExampleMessage>())
            .ok_or("示例问候语缺少原工厂")?;
        assert!(message_binding.shared);
        let original = message_binding
            .concrete
            .invoke::<ExampleMessage>(state.container().factory_resolver()?)
            .await?;
        let decorated = state.resolve::<ExampleMessage>().await?;
        assert_eq!(original.text(), decorated.text());
        assert_eq!(original.binding_extension(), None);
        assert_eq!(decorated.binding_extension(), Some("module-provider"));
        let served_at =
            chrono::DateTime::from_timestamp(946_684_800, 0).ok_or("无法构造固定时间")?;
        let mut parameters = ParameterOverrides::new();
        let _ = parameters.insert(
            ParameterKey::name("served_at"),
            ParameterValue::new(served_at.to_rfc3339()),
        );
        assert_raw_greeting_presenter(
            state.container(),
            parameters.clone(),
            served_at,
            decorated.text(),
        )
        .await?;
        assert!(!state.container().resolved_key(&abstract_key)?);
        assert!(!state.container().resolved_key(&alias)?);
        let named = state
            .resolve_key_with::<dyn GreetingPresenter>(alias.clone(), parameters)
            .await?;
        assert_eq!(named.served_at(), served_at);
        assert!(!state.container().resolved_key(&alias)?);
        let first = state.resolve::<ExamplePresenter>().await?;
        let second = state.resolve::<ExamplePresenter>().await?;
        assert!(!std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(named.text(), first.text());
        assert!(state.container().resolved_key(&alias)?);
        let response = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], first.text());
        let served = json["data"]["served_at"]
            .as_str()
            .ok_or("响应缺少呈现器时间")?;
        assert_ne!(chrono::DateTime::parse_from_rfc3339(served)?, served_at);
        Ok(())
    }
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn production_log_attribute_writes_only_to_the_named_channel()
-> Result<(), Box<dyn std::error::Error>> {
    const MESSAGE: &str = "示例日志请求已处理";
    let directory = tempfile::tempdir()?;
    let selected = directory.path().join("storage/logs/openrust.log");
    let default = directory.path().join("default.log");
    let source = enabled_example_sources(&production_sources(true))?.merge_ordered_entries([
        ("example", serde_json::json!({"logging_demo": true})),
        (
            "logging",
            serde_json::json!({
                "default": "stack",
                "channels": {
                    "stack": {"driver": "stack", "channels": ["default_file"]},
                    "default_file": {"driver": "single", "path": default, "level": "debug"},
                    "single": {
                        "driver": "single",
                        "level": "debug",
                        "replace_placeholders": true
                    },
                    "null": {"driver": "null"}
                }
            }),
        ),
    ])?;
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    let outcome: Result<(), Box<dyn std::error::Error>> = async {
        app.app()?
            .config_repository()
            .set("application_root", serde_json::json!(directory.path()))?;
        let greeting = Request::get("/api/example")
            .header("host", "localhost")
            .body(Body::empty())?;
        app.request(greeting).await?.assert_status(StatusCode::OK)?;
        if selected.exists() {
            assert!(!std::fs::read_to_string(&selected)?.contains(MESSAGE));
        }
        let record = Request::post("/api/example/logging")
            .header("host", "localhost")
            .body(Body::empty())?;
        let response = app.request(record).await?;
        response.assert_status(StatusCode::NO_CONTENT)?;
        assert_eq!(response.body(), b"");
        let logged = std::fs::read_to_string(&selected)?;
        let mut lines = logged.lines().filter(|line| line.contains(MESSAGE));
        let line = lines.next().ok_or("示例请求没有写入日志")?;
        assert!(line.contains("example.request"));
        let (_, extra) = line.split_once(MESSAGE).ok_or("缺少示例日志文案")?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(extra.trim())?,
            serde_json::json!({"example_operation": "logging-demo"}),
        );
        assert!(lines.next().is_none());
        assert!(!logged.contains("example_internal_marker"));
        assert!(!logged.contains("example-private-marker"));
        if default.exists() {
            assert!(!std::fs::read_to_string(&default)?.contains(MESSAGE));
        }
        assert_log_context_placeholders(app.app()?.state().container(), &selected).await?;
        Ok(())
    }
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup?;
    Ok(())
}

async fn assert_log_context_placeholders(
    container: &openrust::container::Container,
    selected: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use openrust::container::ServiceKey;
    use openrust::logging::{LogLevel, LogManager, context::Repository};

    let scope = std::sync::Arc::new(container.scope()?);
    let result = scope
        .run(async {
            let context = scope.resolve::<Repository>().await?;
            context.add("example_operation", serde_json::json!("ordinary-probe"));
            context.add_hidden("probe_hidden", serde_json::json!("hidden-probe"));
            let channel = scope
                .resolve_key::<LogManager>(ServiceKey::named("log"))
                .await?
                .channel(Some("single"))
                .await?;
            channel
                .log(
                    LogLevel::Info,
                    "示例占位检查 {example_operation} {local}",
                    serde_json::Map::from_iter([(
                        "local".into(),
                        serde_json::json!("explicit-probe"),
                    )]),
                )
                .await
        })
        .await;
    scope.close();
    result?;

    let logged = std::fs::read_to_string(selected)?;
    let line = logged
        .lines()
        .find(|line| line.contains("示例占位检查"))
        .ok_or("缺少示例占位检查日志")?;
    let (_, fields) = line
        .split_once("示例占位检查 {example_operation} explicit-probe ")
        .ok_or("日志占位应由显式上下文替换")?;
    let fields = serde_json::Deserializer::from_str(fields)
        .into_iter::<serde_json::Value>()
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        fields,
        [
            serde_json::json!({"local": "explicit-probe"}),
            serde_json::json!({"example_operation": "ordinary-probe"}),
        ],
    );
    assert!(!logged.contains("probe_hidden"));
    assert!(!logged.contains("hidden-probe"));
    Ok(())
}

#[tokio::test]
async fn production_environment_bind_is_registered_by_the_first_http_resolution()
-> Result<(), Box<dyn std::error::Error>> {
    use super::http::controllers::environment_binding::EnvironmentBinding;

    let key = ServiceKey::typed::<dyn EnvironmentBinding>();
    for (environment, implementation) in [
        ("conditional", "conditional"),
        ("testing", "testing"),
        ("production", "fallback"),
    ] {
        let source = enabled_example_sources(&production_sources(true))?
            .merge_ordered_entries([("app", serde_json::json!({"env": environment}))])?;
        let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        assert_eq!(app.app()?.environment().as_str(), environment);
        let container = app.app()?.state().container().clone();
        assert!(!container.contains_key(&key));
        assert!(
            !container
                .get_bindings()?
                .iter()
                .any(|binding| binding.key == key)
        );

        let request = || {
            Request::get("/api/example/environment-binding")
                .header("host", "localhost")
                .body(Body::empty())
        };
        let response = app.request(request()?).await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(
            json["data"],
            serde_json::json!({
                "implementation": implementation,
                "application_visits": 1,
                "request_visits": [1, 2],
            }),
        );
        assert!(container.contains_key(&key));
        assert!(container.resolved_key(&key)?);
        let registered = container
            .get_bindings()?
            .into_iter()
            .find(|binding| binding.key == key)
            .ok_or("首次请求未登记环境绑定工厂")?;
        assert!(!registered.shared);

        let response = app.request(request()?).await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(
            json["data"],
            serde_json::json!({
                "implementation": implementation,
                "application_visits": 2,
                "request_visits": [1, 2],
            }),
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn disposable_example_application_rediscovers_classes_after_flush()
-> Result<(), Box<dyn std::error::Error>> {
    use super::http::controllers::environment_binding::{ApplicationVisits, EnvironmentBinding};
    use openrust::foundation::{Application, ProviderBuilder};
    use std::sync::Arc;

    let source = enabled_example_sources(&production_sources(true))?
        .merge_ordered_entries([("app", serde_json::json!({"env": "testing"}))])?;
    let configuration = source.figment().extract()?;
    let mut builder = ProviderBuilder::for_application::<crate::bootstrap::Server>(&configuration);
    builder.booted(|context| {
        context.require::<dyn EnvironmentBinding>();
        Box::pin(async { Ok(()) })
    });
    let runtime = builder.start(source).await?;
    let mut app = Application::from_provider_runtime(runtime)?;
    let container = app.state().container().clone();
    let key = ServiceKey::typed::<dyn EnvironmentBinding>();
    assert!(container.resolved_key(&key)?);
    assert_eq!(
        container
            .resolve::<dyn EnvironmentBinding>()
            .await?
            .implementation(),
        "testing"
    );
    let previous = container.resolve::<ApplicationVisits>().await?;
    assert!(container.contains_key(&key));

    assert!(app.commands().any(|command| command.name() == "list"));
    app.flush()?;
    assert!(app.commands().next().is_none());
    assert!(!container.contains_key(&key));
    assert!(!container.resolved_key(&key)?);
    assert_eq!(
        container
            .resolve::<dyn EnvironmentBinding>()
            .await?
            .implementation(),
        "testing"
    );
    assert!(container.contains_key(&key));
    assert!(!Arc::ptr_eq(
        &previous,
        &container.resolve::<ApplicationVisits>().await?
    ));
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

async fn assert_raw_greeting_presenter(
    container: &openrust::container::Container,
    parameters: ParameterOverrides,
    served_at: chrono::DateTime<chrono::Utc>,
    expected_text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let binding = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == ServiceKey::typed::<dyn GreetingPresenter>())
        .ok_or("示例问候呈现器缺少类绑定工厂")?;
    let presenter = binding
        .concrete
        .invoke_with::<dyn GreetingPresenter>(container.factory_resolver()?, parameters)
        .await?;
    assert_eq!(presenter.served_at(), served_at);
    assert_eq!(presenter.text(), expected_text);
    Ok(())
}

#[tokio::test]
async fn production_named_greeting_uses_each_request_scope()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        enabled_example_sources(&production_sources(true))?,
    )
    .build()
    .await?;
    let state = app.app()?.state();
    let next_message = Arc::new(AtomicUsize::new(0));
    state
        .container()
        .scoped::<ExampleMessage, _>(move |_| {
            let next_message = Arc::clone(&next_message);
            Box::pin(async move {
                let index = next_message.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(ExampleMessage::new(format!("问候 {index}"))))
            })
        })
        .await?;
    let resolved_presenters = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolved_presenters);
    state.container().after_resolving(
        Some(ServiceKey::typed::<ExamplePresenter>()),
        move |event, _| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                assert_eq!(event.key(), &ServiceKey::typed::<dyn GreetingPresenter>());
                assert!(event.value::<ExamplePresenter>()?.is_some());
                assert!(event.value::<dyn GreetingPresenter>()?.is_some());
                seen.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        },
    )?;
    let root_presenter = state.resolve::<dyn GreetingPresenter>().await?;
    assert_eq!(root_presenter.text(), "问候 0");
    for expected in ["问候 1", "问候 2"] {
        let response = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], expected);
    }
    assert_eq!(resolved_presenters.load(Ordering::Relaxed), 3);
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_greeting_uses_config_named_message_and_lazy_tagged_suffixes()
-> Result<(), Box<dyn std::error::Error>> {
    use super::services::{EXAMPLE_GREETING_SUFFIXES, EXAMPLE_MESSAGE_ALIAS, GreetingSuffix};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let source = enabled_example_sources(&production_sources(true))?.merge_ordered_entries([(
        "example",
        serde_json::json!({"greeting_suffix": "【配置】"}),
    )])?;
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    let state = app.app()?.state();
    let base = state.resolve::<ExampleMessage>().await?;
    assert_eq!(
        app.app()?
            .config_repository()
            .get("example.greeting_suffix")?,
        Some(serde_json::json!("【配置】"))
    );
    let request = || {
        Request::get("/api/example")
            .header("host", "localhost")
            .body(Body::empty())
    };
    let response = app.request(request()?).await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = response.json()?;
    assert_eq!(json["data"]["message"], format!("{}【配置】", base.text()));

    let container = state.container();
    container
        .instance_key(
            ServiceKey::named(EXAMPLE_MESSAGE_ALIAS),
            Arc::new(ExampleMessage::new("命名问候")),
        )
        .await?;
    let suffix_resolutions = Arc::new(AtomicUsize::new(0));
    let first = ServiceKey::named("example.suffix.z");
    let second = ServiceKey::named("example.suffix.a");
    for (key, suffix) in [(first.clone(), "，你好"), (second.clone(), "！")] {
        let suffix_resolutions = Arc::clone(&suffix_resolutions);
        container
            .bind_key::<GreetingSuffix, _>(key, move |_| {
                let suffix_resolutions = Arc::clone(&suffix_resolutions);
                Box::pin(async move {
                    suffix_resolutions.fetch_add(1, Ordering::Relaxed);
                    Ok(Arc::new(GreetingSuffix(suffix)))
                })
            })
            .await?;
    }
    container.tag([first, second], [EXAMPLE_GREETING_SUFFIXES])?;

    let presenter = state.resolve::<dyn GreetingPresenter>().await?;
    assert_eq!(presenter.text(), "命名问候");
    assert!(Arc::ptr_eq(
        &base,
        &state.resolve::<ExampleMessage>().await?
    ));
    assert_eq!(suffix_resolutions.load(Ordering::Relaxed), 0);
    let response = app.request(request()?).await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = response.json()?;
    assert_eq!(json["data"]["message"], "命名问候【配置】，你好！");
    assert_eq!(suffix_resolutions.load(Ordering::Relaxed), 2);

    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_greeting_attributes_supply_time_and_preserve_explicit_override()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracing::instrument::WithSubscriber as _;
    use tracing_subscriber::{Layer, layer::SubscriberExt as _};

    struct GreetingTrace(Arc<AtomicUsize>);

    impl<S: tracing::Subscriber> Layer<S> for GreetingTrace {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if event.metadata().target() == "openrust.example.greeting.attribute" {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    if std::env::var_os(GREETING_TRACE_CHILD).is_none() {
        return run_greeting_trace_in_isolation();
    }

    let constructed = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(GreetingTrace(Arc::clone(&constructed)));
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        enabled_example_sources(&production_sources(true))?,
    )
    .build()
    .await?;
    let outcome = async {
        let state = app.app()?.state();
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&handler_calls);
        let selected = chrono::DateTime::from_timestamp(946_684_800, 0).ok_or("无效的固定时间")?;
        let supplied =
            chrono::DateTime::from_timestamp(1_000_000_000, 0).ok_or("无效的覆盖时间")?;
        state
            .container()
            .when_has_attribute::<ExampleServedAt>(move |_, parameter, _| {
                let seen = Arc::clone(&seen);
                Box::pin(async move {
                    assert_eq!(parameter.name(), "served_at");
                    assert!(!parameter.is_variadic());
                    seen.fetch_add(1, Ordering::Relaxed);
                    Ok(AttributeValue::One(ParameterValue::new(
                        selected.to_rfc3339(),
                    )))
                })
            })?;

        let mut parameters = ParameterOverrides::new();
        let _ = parameters.insert(
            ParameterKey::name("served_at"),
            ParameterValue::new(supplied.to_rfc3339()),
        );
        let presenter = state
            .resolve_with::<dyn GreetingPresenter>(parameters)
            .await?;
        assert_eq!(presenter.served_at(), supplied);
        assert_eq!(handler_calls.load(Ordering::Relaxed), 0);
        assert_eq!(constructed.load(Ordering::Relaxed), 1);

        let response = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], presenter.text());
        let rendered = json["data"]["served_at"]
            .as_str()
            .ok_or("响应缺少呈现器时间")?;
        assert_eq!(chrono::DateTime::parse_from_rfc3339(rendered)?, selected);
        assert_eq!(handler_calls.load(Ordering::Relaxed), 1);
        assert_eq!(constructed.load(Ordering::Relaxed), 2);
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .with_subscriber(subscriber)
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup?;
    Ok(())
}

const GREETING_TRACE_CHILD: &str = "OPENRUST_TEST_ISOLATED_GREETING_TRACE";

fn run_greeting_trace_in_isolation() -> Result<(), Box<dyn std::error::Error>> {
    // tracing 调用点兴趣在进程内共享，其他请求不能参与本测试的计数。
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "modules::example::tests::production_greeting_attributes_supply_time_and_preserve_explicit_override",
            "--nocapture",
        ])
        .env(GREETING_TRACE_CHILD, "1")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.contains("1 passed; 0 failed") {
        return Err(format!(
            "隔离日志回归失败：{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

// 模块通过公开 BootContext 覆盖应用静态 Host 策略，真实业务路由接受或拒绝请求。
#[tokio::test]
async fn module_trusted_hosts_override_reaches_business_routes()
-> Result<(), Box<dyn std::error::Error>> {
    let sources = production_sources(true).merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "server": {"trusted_hosts": {"at": ["^other\\.test$"], "subdomains": false}},
            "example": {"trusted_hosts": {"at": ["^localhost$"], "subdomains": false}}
        }),
    ));
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        enabled_example_sources(&(sources))?,
    )
    .build()
    .await?;
    app.request(
        Request::get("/api/example")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    app.request(
        Request::get("/api/example")
            .header("host", "other.test")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::BAD_REQUEST)?;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

// 真实模块路由经公开异常装配执行 typed render 与最终 Response 定制。
#[tokio::test]
async fn module_exception_handler_renders_typed_error() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        enabled_example_sources(&(production_sources(true)))?,
    )
    .build()
    .await?;
    let response = app
        .request(
            Request::get("/api/example/exception")
                .header("host", "localhost")
                .header("accept", "application/json")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::CONFLICT)?;
    assert_eq!(
        response
            .headers()
            .get("x-example-exception")
            .and_then(|value| value.to_str().ok()),
        Some("rendered")
    );
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["message"], "示例业务冲突");
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

#[cfg(feature = "auth-oidc")]
struct LateOidcOverlay;

#[cfg(feature = "auth-oidc")]
impl openrust::support::ServiceProvider for LateOidcOverlay {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.overlay(
                "example",
                serde_json::json!({"oidc": {
                    "issuer": "http://127.0.0.1:18761/realms/openrust",
                    "client_id": "test-client",
                    "client_secret": null,
                    "redirect_uri": "http://127.0.0.1:3000/api/example/oidc/callback",
                    "allow_loopback": true
                }}),
            )
        })
    }
}

#[cfg(feature = "auth-oidc")]
#[tokio::test]
async fn oidc_overlay_after_module_routes_fails_before_ready()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(openrust::database::DatabaseServiceProvider)
        .provider(super::providers::ExampleServiceProvider::new(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ))
        .provider(LateOidcOverlay);
    let error = builder
        .start(crate::modules::example::test_sources(
            &(production_sources(false)),
        )?)
        .await
        .err()
        .ok_or("late OIDC overlay unexpectedly became ready")?;
    assert_eq!(error.code(), "example_oidc_config_changed_after_routes");
    Ok(())
}

#[tokio::test]
async fn module_registration_controls_only_its_business_routes()
-> Result<(), Box<dyn std::error::Error>> {
    for registered in [false, true] {
        let mut builder = openrust::foundation::ProviderBuilder::new();
        #[cfg(feature = "queue")]
        builder.provider(openrust::queue::QueueServiceProvider::new());
        builder
            .provider(HttpServiceProvider)
            .provider(openrust::http::CookieServiceProvider::plain())
            .provider(openrust::database::DatabaseServiceProvider)
            .provider(openrust::cache::CacheServiceProvider::new())
            .provider(crate::app::providers::AppServiceProvider);
        let manifest = openrust_modules::ModuleManifest::new().module(super::descriptor(
            false,
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ));
        builder.class_descriptors(super::class_descriptors());
        builder.module_manifest(if registered {
            manifest.enable("example")
        } else {
            manifest
        });
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            openrust::config::FrameworkConfig::default(),
        ))
        .merge(figment::providers::Serialized::defaults(
            serde_json::json!({
                "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}}
            }),
        ));
        let app = openrust::Application::from_provider_runtime(
            builder
                .start(crate::modules::example::test_sources(&(sources))?)
                .await?,
        )?;
        assert_eq!(app.environment().as_str(), "production");
        assert!(app.environment_matches(["local", "pro*"]));
        assert!(app.is_production());
        assert!(!app.running_unit_tests());
        assert_eq!(
            app.state()
                .container()
                .contains::<super::services::ExampleMessage>(),
            registered
        );
        if registered {
            assert_eq!(
                app.section::<serde_json::Value>("example")?["greeting"],
                "Hello from modules/example"
            );
        } else {
            assert!(app.section::<serde_json::Value>("example").is_err());
        }
        let router = openrust::foundation::http::build_router(&app)?;
        for path in ["/health", "/api"] {
            let response = router
                .clone()
                .oneshot(
                    Request::get(path)
                        .header("host", "localhost")
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = router
            .oneshot(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if registered {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let response: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(response["success"], registered);
        if registered {
            assert_eq!(response["data"]["message"], "Hello from modules/example");
        } else {
            assert_eq!(response["code"], "not_found");
        }
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
    }
    Ok(())
}

struct LateExampleMessage(&'static str);

impl openrust::support::ServiceProvider for LateExampleMessage {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(super::services::ExampleMessage::new(
                self.0,
            )))
            .await?;
            Ok(())
        })
    }
}

struct StartupExampleMessage(LateExampleMessage);

impl openrust::support::ServiceProvider for StartupExampleMessage {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        openrust::support::ServiceProvider::register(&self.0, app)
    }
}

#[tokio::test]
async fn module_business_uses_runtime_provider_rebinding_after_boot()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = openrust::foundation::ProviderBuilder::new();
    #[cfg(feature = "queue")]
    builder.provider(openrust::queue::QueueServiceProvider::new());
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(openrust::database::DatabaseServiceProvider)
        .provider(openrust::cache::CacheServiceProvider::new())
        .provider(crate::app::providers::AppServiceProvider)
        .provider(StartupExampleMessage(LateExampleMessage(
            "宿主预先登记的问候",
        )))
        .class_descriptors(super::class_descriptors())
        .module_manifest(
            openrust_modules::ModuleManifest::new()
                .module(super::descriptor(
                    false,
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                ))
                .enable("example"),
        );
    let source = figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}}
        }),
    ));
    let mut app = openrust::Application::from_provider_runtime(
        builder
            .start(crate::modules::example::test_sources(&(source))?)
            .await?,
    )?;
    let original = app
        .state()
        .resolve::<super::services::ExampleMessage>()
        .await?;
    assert_eq!(original.text(), "宿主预先登记的问候");
    assert_eq!(original.binding_extension(), Some("module-provider"));
    let original_presenter = app
        .state()
        .resolve_key::<dyn GreetingPresenter>(ServiceKey::named(EXAMPLE_PRESENTER_ALIAS))
        .await?;
    assert_eq!(original_presenter.text(), original.text());
    let first = app
        .register_provider(LateExampleMessage("已登记业务消息"))
        .await?;
    assert_eq!(
        first,
        app.register_provider(LateExampleMessage("重复消息"))
            .await?
    );
    assert_eq!(
        app.state()
            .resolve::<super::services::ExampleMessage>()
            .await?
            .text(),
        "已登记业务消息"
    );
    assert_eq!(
        first,
        app.force_register_provider(LateExampleMessage("强制更新消息"))
            .await?
    );
    assert_eq!(original_presenter.text(), "宿主预先登记的问候");
    assert_eq!(original.text(), "宿主预先登记的问候");
    assert_eq!(
        app.state()
            .resolve::<super::services::ExampleMessage>()
            .await?
            .text(),
        "强制更新消息"
    );
    let updated_presenter = app
        .state()
        .resolve_key::<dyn GreetingPresenter>(ServiceKey::named(EXAMPLE_PRESENTER_ALIAS))
        .await?;
    assert_eq!(updated_presenter.text(), "强制更新消息");
    let response = openrust::foundation::http::build_router(&app)?
        .oneshot(
            Request::get("/api/example")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"]["message"], "强制更新消息");
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

fn production_sources(semantic: bool) -> figment::Figment {
    let mut framework = openrust::config::FrameworkConfig::default();
    if semantic {
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    }
    figment::Figment::from(figment::providers::Serialized::defaults(framework))
        .merge(figment::providers::Serialized::defaults(serde_json::json!({
            "app": {"env": "test"},
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })))
}

async fn assert_initial_config_greeting(
    app: &mut openrust::testing::TestApp<crate::bootstrap::Server>,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .request(
            Request::get("/api/example/config-greeting?key=security.encryption.keys")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        body["data"],
        serde_json::json!({"greeting": "Hello from modules/example"})
    );
    assert!(!String::from_utf8_lossy(response.body()).contains("KioqKioq"));
    Ok(())
}

async fn assert_config_feature(
    app: &mut openrust::testing::TestApp<crate::bootstrap::Server>,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .request(
            Request::get("/api/example/config-feature")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], serde_json::json!({"enabled": true}));
    Ok(())
}

#[tokio::test]
async fn typed_policy_requires_explicit_provider_option() -> Result<(), Box<dyn std::error::Error>>
{
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for enabled in [false, true] {
        let mut builder = openrust::foundation::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(openrust::http::CookieServiceProvider::plain())
            .provider(openrust::database::DatabaseServiceProvider)
            .provider(openrust::cache::CacheServiceProvider::new());
        let provider = super::providers::ExampleServiceProvider::new(root.clone());
        builder.provider(if enabled {
            provider.with_runtime_policy()
        } else {
            provider
        });
        let app = openrust::Application::from_provider_runtime(
            builder
                .start(crate::modules::example::test_sources(
                    &(production_sources(false)),
                )?)
                .await?,
        )?;
        let response = openrust::foundation::http::build_router(&app)?
            .oneshot(
                Request::get("/api/example/config-policy")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if enabled {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        if enabled {
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            let body: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(body["success"], true);
            assert_eq!(body["code"], "ok");
            assert_eq!(body["data"]["greeting_prefix"], "Example");
        }
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn old_toml_fixture_remains_an_explicit_optional_source()
-> Result<(), Box<dyn std::error::Error>> {
    struct Fixture;
    impl openrust::support::ServiceProvider for Fixture {
        fn boot<'a>(
            &'a self,
            app: &'a mut openrust::foundation::BootContext<'_>,
        ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
            Box::pin(async move {
                let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("modules/example/tests/fixtures");
                openrust_modules::ModuleConfigFiles::new("example", root)?.overlay(app)
            })
        }
    }
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(Fixture);
    let app = openrust::Application::from_provider_runtime(
        builder
            .start(crate::modules::example::test_sources(
                &(production_sources(false)),
            )?)
            .await?,
    )?;
    assert_eq!(
        app.config_repository().get("example.greeting")?,
        Some(serde_json::json!("Hello from modules/example"))
    );
    assert_eq!(
        app.config_repository().get("example.feature.enabled")?,
        Some(serde_json::json!(true))
    );
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_config_greeting_reads_only_the_module_business_key()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let repository = openrust::config::repository::ConfigRepository::new(figment::Figment::from(
        figment::providers::Serialized::defaults(serde_json::json!({
            "example": {"greeting": "仓储中的业务问候"}
        })),
    ));
    assert_eq!(
        super::services::config_reading::greeting(&repository)?,
        "仓储中的业务问候"
    );
    let missing = openrust::config::repository::ConfigRepository::new(figment::Figment::from(
        figment::providers::Serialized::defaults(serde_json::json!({"other": true})),
    ));
    assert_eq!(
        super::services::config_reading::greeting(&missing)?,
        "示例问候语尚未配置"
    );
    assert!(!super::services::config_reading::feature_enabled(&missing)?);
    missing.set("example.greeting", serde_json::json!(7))?;
    assert!(super::services::config_reading::greeting(&missing).is_err());

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    assert_config_feature(&mut app).await?;
    let mut other = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let startup_greeting = app
        .app()?
        .state()
        .get::<super::services::ExampleMessage>()
        .ok_or("启动时未解析示例业务服务")?
        .text()
        .to_owned();
    assert_initial_config_greeting(&mut app).await?;
    let reviewed = app
        .request(
            Request::post("/api/example/config-greeting/review?key=security.encryption.keys")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    reviewed.assert_status(StatusCode::OK)?;
    let reviewed_body: serde_json::Value = serde_json::from_slice(reviewed.body())?;
    assert_eq!(
        reviewed_body["data"],
        serde_json::json!({"greeting": "示例问候语已复核"})
    );
    assert_eq!(
        app.app()?.config_repository().get("example.greeting")?,
        Some(serde_json::json!("示例问候语已复核"))
    );
    assert_eq!(
        app.app()?
            .section::<super::services::ExampleConfig>("example")?
            .greeting,
        "示例问候语已复核"
    );
    let refreshed = app
        .request(
            Request::get("/api/example/config-greeting")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    refreshed.assert_status(StatusCode::OK)?;
    let refreshed_body: serde_json::Value = serde_json::from_slice(refreshed.body())?;
    assert_eq!(refreshed_body["data"], reviewed_body["data"]);
    assert_eq!(
        app.app()?
            .state()
            .get::<super::services::ExampleMessage>()
            .ok_or("复核后业务服务意外丢失")?
            .text(),
        startup_greeting
    );
    let independent = other
        .request(
            Request::get("/api/example/config-greeting")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    independent.assert_status(StatusCode::OK)?;
    let independent_body: serde_json::Value = serde_json::from_slice(independent.body())?;
    assert_eq!(
        independent_body["data"],
        serde_json::json!({"greeting": "Hello from modules/example"})
    );
    other
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn ordered_business_source_survives_module_overlay_and_runtime_set()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::config::ConfigLoader;
    use openrust::testing::TestApp;
    use std::time::Duration;

    let defaults: serde_json::Value = production_sources(false).extract()?;
    let defaults = defaults.as_object().ok_or("默认配置根必须是对象")?;
    let mut entries = defaults
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    entries.push((
        "example".to_owned(),
        serde_json::json!({"greeting": "来源中的问候", "article_summary_origin": null}),
    ));
    let sources = ConfigLoader::application_code_sources(
        env!("CARGO_MANIFEST_DIR"),
        "APP_",
        None,
        &<crate::bootstrap::Server as openrust::foundation::ApplicationDefinition>::config(),
    )?
    .merge_ordered_entries(entries)?;
    let mut app = TestApp::<crate::bootstrap::Server>::builder(sources.clone())
        .build()
        .await?;
    let mut other = TestApp::<crate::bootstrap::Server>::builder(sources)
        .build()
        .await?;
    let keys = |app: &TestApp<crate::bootstrap::Server>| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let all = app.app()?.config_repository().all()?;
        let section = all["example"].as_object().ok_or("example 区段必须是对象")?;
        Ok(section.keys().cloned().collect())
    };
    let first_two = |app: &TestApp<crate::bootstrap::Server>| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(keys(app)?.into_iter().take(2).collect())
    };
    assert_eq!(first_two(&app)?, ["greeting", "article_summary_origin"]);
    assert_eq!(
        app.app()?.config_repository().get("example.greeting")?,
        Some(serde_json::json!("Hello from modules/example"))
    );
    let feature = app
        .request(
            Request::get("/api/example/config-feature")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    feature.assert_status(StatusCode::OK)?;
    let feature_body: serde_json::Value = serde_json::from_slice(feature.body())?;
    assert_eq!(feature_body["data"], serde_json::json!({"enabled": true}));
    app.app()?
        .config_repository()
        .set("example.greeting", serde_json::json!("当前应用已复核"))?;
    assert_eq!(first_two(&app)?, ["greeting", "article_summary_origin"]);
    assert_eq!(
        app.app()?
            .section::<super::services::ExampleConfig>("example")?
            .greeting,
        "当前应用已复核"
    );
    let response = app
        .request(
            Request::get("/api/example/config-greeting")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        body["data"],
        serde_json::json!({"greeting": "当前应用已复核"})
    );
    assert_eq!(
        other.app()?.config_repository().get("example.greeting")?,
        Some(serde_json::json!("Hello from modules/example"))
    );
    assert_eq!(first_two(&other)?, ["greeting", "article_summary_origin"]);
    other
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_representation_negotiates_html_json_and_rejection()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let sources = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({"api": {"http_status": "always_ok"}}),
    ));
    let mut app =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources))?)
            .build()
            .await?;
    for (accept, status, content_type) in [
        (None, StatusCode::OK, "text/html"),
        (Some(""), StatusCode::OK, "text/html"),
        (Some("text/html"), StatusCode::OK, "text/html"),
        (Some("application/json"), StatusCode::OK, "application/json"),
        (
            Some("text/html;q=0.2, application/json;q=0.9"),
            StatusCode::OK,
            "application/json",
        ),
        (
            Some("text/html;q=0.9, application/json;q=0.2"),
            StatusCode::OK,
            "text/html",
        ),
        (Some("text/plain"), StatusCode::OK, "application/json"),
    ] {
        let mut request = Request::get("/api/example/representation").header("host", "localhost");
        if let Some(accept) = accept {
            request = request.header("accept", accept);
        }
        let response = app.request(request.body(Body::empty())?).await?;
        assert_eq!(
            response.status(),
            status,
            "Accept {accept:?}; actual HTTP status: {}",
            response.status()
        );
        assert_eq!(response.headers()["vary"], "Accept");
        assert!(
            response.headers()["content-type"]
                .to_str()?
                .starts_with(content_type),
            "Accept {accept:?}"
        );
        if content_type == "text/html" {
            let html = std::str::from_utf8(response.body())?;
            assert!(html.contains("Hello from modules/example"));
        } else {
            let json: serde_json::Value = serde_json::from_slice(response.body())?;
            if accept == Some("text/plain") {
                assert_eq!(json["code"], "not_acceptable");
            } else {
                assert_eq!(json["data"]["message"], "Hello from modules/example");
            }
        }
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_representation_uses_semantic_406_when_configured()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(true)),
    )?)
    .build()
    .await?;
    let response = app
        .request(
            Request::get("/api/example/representation")
                .header("host", "localhost")
                .header("accept", "text/plain")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::NOT_ACCEPTABLE)?;
    assert_eq!(response.headers()["vary"], "Accept");
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["code"], "not_acceptable");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_info_leaves_json_body_for_typed_extractor()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let response = app
        .request(
            Request::post("/api/example/request-body")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"完整 JSON 正文 <>&"}"#))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["data"]["path"], "api/example/request-body");
    assert_eq!(json["data"]["method"], "POST");
    assert_eq!(json["data"]["is_json"], true);
    assert_eq!(json["data"]["title"], "完整 JSON 正文 <>&");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_html_representation_escapes_business_message()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::{sync::Arc, time::Duration};

    let mut builder = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?);
    builder.configure_after_register(|bindings| {
        Box::pin(async move {
            bindings
                .instance(Arc::new(super::services::ExampleMessage::new(
                    "<script>alert('x' & \"y\")</script>",
                )))
                .await?;
            Ok(())
        })
    });
    let mut app = builder.build().await?;
    let response = app
        .request(
            Request::get("/api/example/representation")
                .header("host", "localhost")
                .header("accept", "text/html")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(response.body())?;
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39; &amp; &quot;y&quot;)&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_testapp_replaces_business_dependency_without_changing_routes()
-> Result<(), Box<dyn std::error::Error>> {
    use super::services::ExampleMessage;
    use openrust::testing::TestApp;
    use std::sync::Arc;
    use std::time::Duration;

    let mut production = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let mut fixture = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?);
    fixture.configure_after_register(|bindings| {
        Box::pin(async move {
            bindings
                .instance(Arc::new(ExampleMessage::new(
                    "A test-owned business message",
                )))
                .await?;
            Ok(())
        })
    });
    let mut replaced = fixture.build().await?;
    for (app, expected) in [
        (&production, "Hello from modules/example"),
        (&replaced, "A test-owned business message"),
    ] {
        let response = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(json["data"]["message"], expected);
        assert!(json["data"]["served_at"].as_str().is_some());
    }
    let translated = production
        .request(
            Request::get("/api/example/heartbeat-label")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    translated.assert_status(StatusCode::OK)?;
    let translated: serde_json::Value = translated.json()?;
    assert_eq!(translated["data"]["message"], "Heartbeat completed");
    assert_eq!(translated["data"]["locale"], "en");
    assert!(translated["data"]["number"].as_str().is_some());
    let chinese = production
        .request(
            Request::get("/api/example/heartbeat-label")
                .header("Accept-Language", "zh-CN")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    let chinese: serde_json::Value = chinese.json()?;
    assert_eq!(chinese["data"]["locale"], "zh-CN");
    assert_eq!(chinese["data"]["message"], "心跳任务已完成");
    production
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    replaced
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_module_domain_route_binds_host_path_and_named_url()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let response = app
        .request(
            Request::get("/api/example/domain/orders/a%20b")
                .header("host", "Acme.example.test")
                .header("x-forwarded-host", "evil.test")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = response.json()?;
    assert_eq!(json["data"]["account"], "acme");
    assert_eq!(json["data"]["id"], "a b");
    assert_eq!(
        json["data"]["url"],
        "http://acme.example.test:3000/api/example/domain/orders/a%20b"
    );
    let wrong_host = app
        .request(
            Request::get("/api/example/domain/orders/a%20b")
                .header("host", "unrelated.test")
                .body(Body::empty())?,
        )
        .await?;
    wrong_host.assert_status(StatusCode::NOT_FOUND)?;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_testapp_can_disable_only_the_business_module()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut fixture = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(true)),
    )?);
    fixture.remove_module("example");
    let mut app = fixture.build().await?;
    let state = app.app()?.state();
    let alias = ServiceKey::named(EXAMPLE_PRESENTER_ALIAS);
    assert!(!state.container().contains_key(&alias));
    assert!(matches!(
        state.resolve_key::<dyn GreetingPresenter>(alias).await,
        Err(error) if error.code() == "service_not_bound"
    ));
    app.request(
        Request::get("/api/example")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::NOT_FOUND)?;
    app.request(
        Request::get("/health")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct OrdersConsumerProvider;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
impl openrust::support::ServiceProvider for OrdersConsumerProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.resolve::<super::services::ExampleMessage>().await?;
            app.routes(|routes| routes.get("/api/orders", orders_index))?;
            Ok(())
        })
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn orders_index(
    openrust::http::Inject(message): openrust::http::Inject<super::services::ExampleMessage>,
) -> openrust::http::Json<serde_json::Value> {
    openrust::http::Json(serde_json::json!({"producer": message.text()}))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
async fn dependent_business_module_serves_http_with_real_database()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, Statement};
    use openrust::foundation::ProviderBuilder;
    use openrust::testing::TestDatabase;
    use openrust_modules::{ModuleDescriptor, ModuleManifest};
    use std::path::PathBuf;
    use std::time::Duration;

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping dependent module HTTP test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let consumer_root = tempfile::tempdir()?;
    let application_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sources = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({"database": {"url": database.url()}}),
    ));

    let mut builder = ProviderBuilder::new();
    #[cfg(feature = "queue")]
    builder.provider(openrust::queue::QueueServiceProvider::new());
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(openrust::database::DatabaseServiceProvider)
        .provider(openrust::cache::CacheServiceProvider::new())
        .class_descriptors(super::class_descriptors())
        .module_manifest(
            ModuleManifest::new()
                .module(super::descriptor(false, application_root))
                .module(
                    ModuleDescriptor::new("orders", consumer_root.path(), -100)
                        .requires("example")
                        .provider(OrdersConsumerProvider),
                )
                .enable("orders")
                .enable("example"),
        );
    let runtime = builder
        .start(crate::modules::example::test_sources(&(sources))?)
        .await?;
    let app = openrust::foundation::Application::from_provider_runtime(runtime)?;
    let database_handle = app.state().resolve::<openrust::Db>().await?;
    let connection = database_handle.primary_connection()?;
    connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT 1".to_owned(),
        ))
        .await?
        .ok_or("real database did not return SELECT 1")?;

    let router = openrust::foundation::http::build_router(&app)?;
    for (path, expected) in [
        ("/api/example", "Hello from modules/example"),
        ("/api/orders", "Hello from modules/example"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        if path == "/api/example" {
            assert_eq!(json["data"]["message"], expected);
        } else {
            assert_eq!(json["data"]["producer"], expected);
        }
    }
    #[cfg(feature = "session")]
    assert!(
        !app.state()
            .container()
            .contains::<openrust::session::SessionStoreHandle>()
    );
    let unavailable = router
        .oneshot(
            Request::post("/api/example/account/session/login")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(unavailable.status(), StatusCode::NOT_FOUND);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}

#[cfg(feature = "view")]
#[tokio::test]
async fn production_testapp_renders_module_namespaced_view()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let application_views = tempfile::tempdir()?;
    let source = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "app": {"key":"base64:KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="},
            "view": {"directory": application_views.path(), "auto_reload": false},
            "session": {"driver": "array", "secure": false}
        }),
    ));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    )
    .build()
    .await?;
    let response = app
        .request(
            Request::get("/example/pages/about")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(response.body())?;
    assert!(html.contains("关于示例应用"));
    assert!(html.contains("注入 <code>Views</code>，并通过 <code>pages/about.html</code>"));
    assert!(!html.contains("&lt;code&gt;Views"));
    assert!(html.contains("标题：</p>"));
    assert!(html.contains("摘要：</p>"));
    assert!(html.contains("不存在的字段：</p>"));
    assert!(!html.contains("<ul>"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
struct EmptyMigrator;

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl openrust::database::sea_orm_migration::MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn module_migration_sources(database: &openrust::testing::TestDatabase) -> figment::Figment {
    production_sources(false).merge(figment::providers::Serialized::defaults(
            serde_json::json!({
                "database": {"url": database.url()},
                "auth": {"jwt": {
                    "issuer": "http://openrust.test", "audience": "module-migration-test",
                    "active_key_id": "test", "keys": {"test": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI="}
                }},
                "security": {"signed_links": {
                    "issuer": "http://openrust.test", "active_key_id": "test",
                    "keys": {"test": "Q0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0M="}
                }}
            }),
        ))
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
async fn migration_versions(
    connection: &openrust::database::DatabaseConnection,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, Statement};
    connection
        .query_all_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT version FROM openrust_migrations ORDER BY version".to_owned(),
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "version").map_err(Into::into))
        .collect()
}
#[cfg(all(
    feature = "session",
    any(feature = "db-postgres", feature = "db-mysql")
))]
async fn session_gc_indexes(
    connection: &openrust::database::DatabaseConnection,
    table: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};
    let expected = format!("{table}_last_activity_index")
        .to_ascii_lowercase()
        .replace(['-', '.'], "_");
    let sql = match connection.get_database_backend() {
        DbBackend::Postgres => {
            "SELECT i.relname AS name FROM pg_index x JOIN pg_class i ON i.oid=x.indexrelid JOIN pg_class t ON t.oid=x.indrelid JOIN pg_namespace n ON n.oid=t.relnamespace JOIN pg_am m ON m.oid=i.relam JOIN pg_attribute a ON a.attrelid=t.oid AND a.attnum=x.indkey[0] WHERE n.nspname=current_schema() AND t.relname=$1 AND i.relname=$2 AND m.amname='btree' AND NOT x.indisunique AND x.indisvalid AND x.indisready AND x.indnatts=1 AND x.indnkeyatts=1 AND x.indpred IS NULL AND x.indexprs IS NULL AND a.attname='last_activity' ORDER BY i.relname"
        }
        DbBackend::MySql => {
            "SELECT index_name AS name FROM information_schema.statistics WHERE table_schema=DATABASE() AND table_name=? AND index_name=? GROUP BY index_name HAVING COUNT(*)=1 AND MIN(seq_in_index)=1 AND MAX(seq_in_index)=1 AND MIN(non_unique)=1 AND MAX(non_unique)=1 AND MIN(index_type)='BTREE' AND MAX(index_type)='BTREE' AND MAX(column_name)='last_activity' AND MAX(sub_part) IS NULL ORDER BY index_name"
        }
        _ => return Err("不支持的迁移测试数据库".into()),
    };
    connection
        .query_all_raw(Statement::from_sql_and_values(
            connection.get_database_backend(),
            sql,
            [table.into(), expected.into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "name").map_err(Into::into))
        .collect()
}

#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
#[tokio::test]
#[allow(clippy::too_many_lines)] // 同一隔离库验证启用、迁移与禁用后历史留存。
async fn module_migration_runs_with_framework_and_disable_keeps_existing_table()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::database::sea_orm::{ConnectionTrait, Statement};
    use openrust::testing::{TestApp, TestDatabase};
    use std::time::Duration;

    fn migration_command(
        phase: &'static str,
        result: openrust::Result<()>,
    ) -> openrust::Result<()> {
        result.inspect_err(|error| {
            eprintln!(
                "module_migration phase={phase} code={} status={}",
                error.code(),
                error.status()
            );
        })
    }

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping module migration test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    eprintln!("module_migration phase=setup");
    let database = TestDatabase::create::<EmptyMigrator>(&admin_url)
        .await
        .inspect_err(|error| {
            eprintln!(
                "module_migration phase=create_database code={} status={}",
                error.code(),
                error.status()
            );
        })?;
    eprintln!("module_migration phase=database_created");
    let sources = || module_migration_sources(&database);
    migration_command(
        "initial_migrate",
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources()))?)
            .run_command("migrate", Vec::new())
            .await,
    )?;
    let mut enabled =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources()))?)
            .build()
            .await
            .inspect_err(|error| {
                eprintln!(
                    "module_migration phase=build_app code={} status={}",
                    error.code(),
                    error.status()
                );
            })?;
    eprintln!("module_migration phase=read_initial");
    let connection = database.connection()?;
    let table = |name| {
        Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT id FROM {name} LIMIT 1"),
        )
    };
    let statement = table("example_heartbeats");
    let articles = table("example_articles");
    let article_events = table("example_article_events");
    let showcase_categories = table("example_record_categories");
    let showcase_records = table("example_records");
    let showcase_links = Statement::from_string(
        connection.get_database_backend(),
        "SELECT record_id FROM example_record_category_links LIMIT 1".to_owned(),
    );
    let attachment_cleanup = Statement::from_string(
        connection.get_database_backend(),
        "SELECT article_id FROM example_attachment_cleanup LIMIT 1".to_owned(),
    );
    let article_imports = Statement::from_string(
        connection.get_database_backend(),
        "SELECT import_id FROM example_article_imports LIMIT 1".to_owned(),
    );
    let private_notes = Statement::from_string(
        connection.get_database_backend(),
        "SELECT private_note_ciphertext FROM example_articles LIMIT 1".to_owned(),
    );
    connection.query_one_raw(statement.clone()).await?;
    connection.query_one_raw(articles.clone()).await?;
    connection.query_one_raw(article_events.clone()).await?;
    connection.query_one_raw(private_notes.clone()).await?;
    connection.query_one_raw(attachment_cleanup.clone()).await?;
    connection.query_one_raw(article_imports.clone()).await?;
    connection
        .query_one_raw(showcase_categories.clone())
        .await?;
    connection.query_one_raw(showcase_records.clone()).await?;
    connection.query_one_raw(showcase_links.clone()).await?;
    let shared_history = Statement::from_string(
        connection.get_database_backend(),
        "SELECT version FROM openrust_migrations".to_owned(),
    );
    let before_history = migration_versions(&connection).await?;
    let before_down = before_history.len();
    #[cfg(feature = "session")]
    let gc_indexes = {
        use openrust::database::sea_orm_migration::{MigrationName, MigratorTrait};
        let gc_position = before_history
            .iter()
            .position(|name| name == openrust::session::SessionGcMigration.name())
            .ok_or("Session GC迁移未登记")?;
        let gc_name = openrust::session::SessionGcMigration.name();
        // 共享历史按版本排序，框架、应用和模块都可能在 GC 之后登记迁移。
        let mut later_migrations = crate::database::Migrator::migrations()
            .into_iter()
            .chain(crate::modules::example::database::migrations::Migrator::migrations())
            .map(|migration| migration.name().to_owned())
            .filter(|name| name.as_str() > gc_name)
            .collect::<Vec<_>>();
        later_migrations.sort();
        assert_eq!(
            &before_history[gc_position + 1..],
            later_migrations.as_slice()
        );
        let gc_down_steps = before_down - gc_position;
        let db = enabled
            .app()?
            .state()
            .resolve::<openrust::database::Db>()
            .await?;
        let indexes = session_gc_indexes(&connection, &db.table_name("sessions")?).await?;
        assert_eq!(indexes.len(), 1);
        migration_command(
            "session_gc_down",
            enabled
                .app()?
                .run_command(
                    "migrate",
                    vec!["down".to_owned(), gc_down_steps.to_string()],
                )
                .await,
        )?;
        eprintln!("module_migration phase=read_after_session_gc_down");
        assert_eq!(
            migration_versions(&connection).await?,
            before_history[..gc_position]
        );
        assert_eq!(
            session_gc_indexes(&connection, &db.table_name("sessions")?).await?,
            Vec::<String>::new()
        );
        connection
            .query_one_raw(showcase_categories.clone())
            .await?;
        connection.query_one_raw(showcase_records.clone()).await?;
        assert!(
            connection
                .query_one_raw(showcase_links.clone())
                .await
                .is_err()
        );
        connection.query_one_raw(statement.clone()).await?;
        connection.query_one_raw(articles.clone()).await?;
        connection.query_one_raw(article_events.clone()).await?;
        connection.query_one_raw(private_notes.clone()).await?;
        connection.query_one_raw(attachment_cleanup.clone()).await?;
        connection.query_one_raw(article_imports.clone()).await?;
        indexes
    };
    let business_history = migration_versions(&connection).await?;
    let showcase_position = business_history
        .iter()
        .position(|name| name == "m20261029_000009_example_record_showcase")
        .ok_or("示例记录迁移未登记")?;
    // workspace 成员可能为框架启用额外 feature，按实际历史回退到示例版本之前。
    let business_steps = business_history.len() - showcase_position;
    migration_command(
        "business_down",
        enabled
            .app()?
            .run_command(
                "migrate",
                vec!["down".to_owned(), business_steps.to_string()],
            )
            .await,
    )?;
    eprintln!("module_migration phase=read_after_business_down");
    assert!(
        connection
            .query_one_raw(showcase_links.clone())
            .await
            .is_err()
    );
    connection.query_one_raw(statement.clone()).await?;
    connection.query_one_raw(articles.clone()).await?;
    connection.query_one_raw(article_events.clone()).await?;
    connection.query_one_raw(private_notes.clone()).await?;
    connection.query_one_raw(attachment_cleanup.clone()).await?;
    connection.query_one_raw(article_imports.clone()).await?;
    assert!(
        connection
            .query_one_raw(showcase_records.clone())
            .await
            .is_err()
    );
    assert!(
        connection
            .query_one_raw(showcase_categories.clone())
            .await
            .is_err()
    );
    assert_eq!(
        connection
            .query_all_raw(shared_history.clone())
            .await?
            .len()
            + business_steps,
        business_history.len()
    );
    assert_eq!(
        migration_versions(&connection).await?,
        business_history[..showcase_position]
    );
    migration_command(
        "business_up",
        enabled
            .app()?
            .run_command("migrate", vec!["up".to_owned(), business_steps.to_string()])
            .await,
    )?;
    eprintln!("module_migration phase=read_after_business_up");
    connection.query_one_raw(statement.clone()).await?;
    connection.query_one_raw(articles.clone()).await?;
    connection.query_one_raw(article_events.clone()).await?;
    connection.query_one_raw(private_notes.clone()).await?;
    connection.query_one_raw(attachment_cleanup.clone()).await?;
    connection.query_one_raw(article_imports.clone()).await?;
    connection
        .query_one_raw(showcase_categories.clone())
        .await?;
    connection.query_one_raw(showcase_records.clone()).await?;
    assert_eq!(migration_versions(&connection).await?, business_history);
    #[cfg(feature = "session")]
    {
        let db = enabled
            .app()?
            .state()
            .resolve::<openrust::database::Db>()
            .await?;
        assert_eq!(
            session_gc_indexes(&connection, &db.table_name("sessions")?).await?,
            Vec::<String>::new()
        );
        migration_command(
            "session_gc_up",
            enabled
                .app()?
                .run_command("migrate", vec!["up".to_owned(), "1".to_owned()])
                .await,
        )?;
        eprintln!("module_migration phase=read_after_session_gc_up");
        assert_eq!(
            session_gc_indexes(&connection, &db.table_name("sessions")?).await?,
            gc_indexes
        );
        connection
            .query_one_raw(showcase_categories.clone())
            .await?;
        connection.query_one_raw(showcase_records.clone()).await?;
        let remaining = before_down - migration_versions(&connection).await?.len();
        if remaining > 0 {
            migration_command(
                "later_migrations_up",
                enabled
                    .app()?
                    .run_command("migrate", vec!["up".to_owned(), remaining.to_string()])
                    .await,
            )?;
        }
    }
    connection.query_one_raw(showcase_links.clone()).await?;
    assert_eq!(migration_versions(&connection).await?, before_history);
    assert_eq!(
        connection.query_all_raw(shared_history).await?.len(),
        before_down
    );
    eprintln!("module_migration phase=shutdown_enabled");
    enabled
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;

    let mut disabled =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources()))?);
    disabled.remove_module("example");
    let mut disabled = disabled.build().await?;
    migration_command(
        "disabled_migrate",
        disabled.app()?.run_command("migrate", Vec::new()).await,
    )?;
    eprintln!("module_migration phase=read_disabled");
    connection.query_one_raw(statement).await?;
    connection.query_one_raw(articles).await?;
    connection.query_one_raw(article_events).await?;
    connection.query_one_raw(private_notes).await?;
    connection.query_one_raw(attachment_cleanup).await?;
    connection.query_one_raw(article_imports).await?;
    connection.query_one_raw(showcase_categories).await?;
    connection.query_one_raw(showcase_records).await?;
    connection.query_one_raw(showcase_links).await?;
    disabled
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn production_input_normalizes_json_and_preserves_source_and_raw_content()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let body = serde_json::json!({
        "title": "  body title  ", "profile": {"name": "  Alice  "}, "nullable": "  ",
        "password": "  private-sentinel  ", "user": {"password": "  nested-sentinel  "},
        "password_confirmation": "",
        "typed": {
            "title":"  demo  ", "quantity":"12", "amount":"1.25x", "subscribed":"YES",
            "publication":"published", "publications":{"2":"draft","5":"unknown","9":"published"},
            "items":{"2":"a","5":"b"}, "profile":{"display_name":" Alice "}, "note":"0",
            "published_at":"2020-01-01T16:30:25+00:00", "calendar_date":"2020-01-02",
            "duration":"90 minutes", "delay":75
        }
    })
    .to_string();
    let response = app
        .request(
            Request::post(
                "/api/example/input?title=query&nullable=from-query&fallback=%20query-only%20&page=7&label=%20query%20",
            )
            .header("host", "localhost")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    let data = &json["data"];
    assert_eq!(
        data["selected"],
        serde_json::json!({"title": "body title", "profile": {"name": "Alice"}})
    );
    assert_eq!(
        data["typed"],
        serde_json::json!({
            "title":"demo", "quantity":12, "amount":1.25, "subscribed":true,
            "bounded_quantity":10, "publication":"published", "publications":{"2":"draft","9":"published"},
            "items":{"2":"a","5":"b"}, "selected":{"typed":{"title":"demo","quantity":"12"}},
            "profile_display_name":"Alice", "note_produced_value":false,
            "published_at":"2020-01-01T16:30:25+00:00", "calendar_date":"2020-01-02T00:00:00+00:00",
            "duration":{"years":0,"months":0,"days":0,"hours":0,"minutes":90,"seconds":0,"microseconds":0,"invert":false,"total_days":null},
            "delay_minutes":75
        })
    );
    assert_eq!(data["title"], "body title");
    assert_eq!(data["profile_name"], "Alice");
    assert_eq!(data["query_title"], "query");
    assert_eq!(data["json_title"], "body title");
    assert!(data["form_title"].is_null());
    assert_eq!(data["query_page"], 7);
    assert_eq!(data["query_label"], "query");
    assert_eq!(
        data["checks"],
        serde_json::json!({
            "has_nullable": true, "nullable_filled": false, "has_any": true, "missing": true,
            "is_not_filled": true, "any_filled": true, "password_has_outer_spaces": true,
            "nested_password_has_outer_spaces": false, "empty_password_is_null": true
        })
    );
    assert_eq!(data["excluded_title"], true);
    assert_eq!(data["source_has_title"], true);
    assert_eq!(
        data["merged"],
        serde_json::json!({"title": "  revised  ", "profile": {"role": "reader"}, "generated": "fallback"})
    );
    assert_eq!(data["replaced_title"], "replacement");
    assert_eq!(data["query_fallback"], "query-only");
    assert_eq!(data["content_bytes"], body.len());
    assert_eq!(data["content_preserved"], true);
    assert!(!std::str::from_utf8(response.body())?.contains("sentinel"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_input_parses_php_form_arrays_and_mixed_keys()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let response = app.request(
        Request::post("/api/example/input?title=query&fallback=still-query")
            .header("host", "localhost")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("title=%20form%20&profile[name]=%20Bob%20&items[]=%20first%20&items[]=second&php_keys[-1]=negative&php_keys[00]=leading&php_keys[2]=sparse&nullable="))?
    ).await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        json["data"]["selected"],
        serde_json::json!({
            "title": "form", "profile": {"name": "Bob"}, "items": ["first", "second"],
            "php_keys": {"-1": "negative", "00": "leading", "2": "sparse"}
        })
    );
    assert_eq!(json["data"]["form_title"], "form");
    assert_eq!(json["data"]["query_title"], "query");
    assert_eq!(json["data"]["checks"]["has_nullable"], true);
    assert_eq!(json["data"]["checks"]["nullable_filled"], false);
    assert_eq!(json["data"]["query_fallback"], "still-query");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_input_raw_policy_is_scoped_and_typed_json_keeps_default_normalization()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    for (path, expected) in [
        ("/api/example/input-raw", "  title  "),
        ("/api/example/request-body", "title"),
        ("/api/example/input-raw", "  title  "),
    ] {
        let response = app
            .request(
                Request::post(path)
                    .header("host", "localhost")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"  title  ","nullable":""}"#))?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        assert_eq!(json["data"]["title"], expected);
        if path.ends_with("input-raw") {
            assert_eq!(json["data"]["nullable"], "");
        }
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

// 捕获原始 URI 被 GET 输入修改覆盖、POST 正文泄入查询以及敏感查询被回显的回归。
#[tokio::test]
async fn production_request_url_uses_original_uri_and_current_query_bag()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;
    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    let base = "http://localhost:8080/api/example/request-url";
    let original = format!("{base}?label=public&page=1");
    for (method, json_source) in [("GET", false), ("GET", true), ("POST", true)] {
        let mut builder = Request::builder()
            .method(method)
            .uri("/api/example/request-url?page=1&label=public")
            .header("host", "localhost:8080")
            .header("purpose", "prefetch")
            .header("authorization", "Bearer never-echo")
            .header("cookie", "private=never-echo");
        if json_source {
            builder = builder.header("content-type", "application/json");
        }
        let response = app
            .request(builder.body(if json_source {
                Body::from(r#"{"secret":"never-echo"}"#)
            } else {
                Body::empty()
            })?)
            .await?;
        let mutates_query = method == "GET" && !json_source;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        let data = &json["data"];
        assert_eq!(data["address"]["root"], "http://localhost:8080");
        assert_eq!(data["address"]["url"], base);
        assert_eq!(data["address"]["full_url"], original);
        assert_eq!(data["address"]["scheme"], "http");
        assert_eq!(data["address"]["http_host"], "localhost:8080");
        assert_eq!(data["address"]["port"], 8080);
        assert_eq!(data["address"]["base_url"], "");
        assert_eq!(data["address"]["client_ip"], serde_json::Value::Null);
        assert_eq!(data["address"]["client_ips"], serde_json::json!([]));
        assert_eq!(data["address"]["decoded_path"], "api/example/request-url");
        assert_eq!(
            data["address"]["segments"],
            serde_json::json!(["api", "example", "request-url"])
        );
        assert_eq!(data["address"]["first_segment"], "api");
        assert_eq!(data["address"]["zero_segment"], serde_json::Value::Null);
        assert_eq!(data["address"]["missing_segment"], serde_json::Value::Null);
        assert_eq!(data["address"]["is_example"], true);
        assert_eq!(data["address"]["full_url_is_example"], true);
        assert_eq!(data["address"]["prefetch"], true);
        assert_eq!(
            data["address"]["with_query"],
            format!("{base}?page=2&label=public")
        );
        assert_eq!(
            data["address"]["without_query"],
            format!("{base}?label=public")
        );
        assert_eq!(data["merged"]["full_url"], original);
        assert_eq!(data["replaced_full_url"], original);
        let page = if mutates_query { 3 } else { 1 };
        assert_eq!(
            data["merged"]["with_query"],
            format!("{base}?page={page}&label=public&view=compact")
        );
        assert_eq!(
            data["merged"]["without_query"],
            format!("{base}?page={page}")
        );
        let replacement = if mutates_query {
            format!("{base}?page=4&view=compact")
        } else {
            format!("{base}?page=1&label=public&view=compact")
        };
        assert_eq!(data["replaced_with_query"], replacement);
        assert!(!String::from_utf8_lossy(response.body()).contains("never-echo"));
    }
    let response = app
        .request(
            Request::get("/api/example/request-url?token=never-echo")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::BAD_REQUEST)?;
    assert!(!String::from_utf8_lossy(response.body()).contains("never-echo"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

// 证明普通业务模块读取的是公开可信代理上下文，不自行解析转发头。
#[tokio::test]
async fn production_request_url_reads_trusted_proxy_context()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::http::{TrustedProxyHeaders, axum::extract::connect_info::ConnectInfo};
    use openrust::testing::TestApp;
    use std::{net::SocketAddr, time::Duration};

    let sources = production_sources(false).merge(figment::providers::Serialized::defaults(
        serde_json::json!({"server": {
            "trusted_proxy_ranges": ["10.0.0.0/8"],
            "trusted_proxy_headers": TrustedProxyHeaders::DEFAULT
        }}),
    ));
    let mut app =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources))?)
            .build()
            .await?;
    let response = app
        .request(
            Request::get("/api/example/request-url?label=public&page=1")
                .header("host", "internal.test:8080")
                .header("x-forwarded-for", "198.51.100.8")
                .header("x-forwarded-host", "public.test")
                .header("x-forwarded-port", "8443")
                .header("x-forwarded-proto", "https")
                .header("x-forwarded-prefix", "/gateway")
                .extension(ConnectInfo("10.0.0.2:42000".parse::<SocketAddr>()?))
                .body(Body::empty())?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    let address = &json["data"]["address"];
    assert_eq!(address["client_ip"], "198.51.100.8");
    assert_eq!(address["client_ips"], serde_json::json!(["198.51.100.8"]));
    assert_eq!(address["base_url"], "/gateway");
    assert_eq!(address["scheme"], "https");
    assert_eq!(address["http_host"], "public.test:8443");
    assert_eq!(address["port"], 8443);
    assert_eq!(address["root"], "https://public.test:8443/gateway");
    assert_eq!(
        address["full_url"],
        "https://public.test:8443/gateway/api/example/request-url?label=public&page=1"
    );
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

// 捕获模块装配丢失路由名、参数被输入覆盖，以及业务错误或 middleware 短路抹去匹配上下文。
#[tokio::test]
async fn production_route_context_keeps_matched_identity_across_business_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;
    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &(production_sources(false)),
    )?)
    .build()
    .await?;
    for (kind, named) in [("named", true), ("anonymous", false)] {
        let response = app
            .request(
                Request::get(format!(
                    "/api/example/route-context/{kind}/42?id=query&absent=query"
                ))
                .header("host", "localhost")
                .header("x-route-name", "forged")
                .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        let data = &json["data"];
        assert_eq!(
            data["name"],
            if named {
                serde_json::json!("example.route-context.show")
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(data["is_example"], named);
        assert_eq!(
            data["uri"],
            format!("api/example/route-context/{kind}/{{id}}")
        );
        assert_eq!(data["id"], "42");
        assert_eq!(data["absent"], serde_json::Value::Null);
        assert_eq!(data["default"], "public-default");
        assert_eq!(data["parameters"], serde_json::json!([["id", "42"]]));
        assert_eq!(
            data["methods"],
            if named {
                serde_json::json!(["GET", "HEAD", "POST"])
            } else {
                serde_json::json!(["GET", "HEAD"])
            }
        );
    }
    for (id, status) in [
        ("missing", StatusCode::NOT_FOUND),
        ("blocked", StatusCode::FORBIDDEN),
    ] {
        let response = app
            .request(
                Request::get(format!("/api/example/route-context/named/{id}"))
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(status)?;
        for header in ["x-example-route-before", "x-example-route-after"] {
            assert_eq!(response.headers()[header], "example.route-context.show");
        }
    }
    let response = app
        .request(
            Request::post("/api/example/route-context/named/43?id=query")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"id":"body","absent":"body"}"#))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["data"]["id"], "43");
    assert_eq!(json["data"]["default"], "public-default");
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn production_request_attributes_use_decoded_path_and_trusted_values()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::http::axum::http::Method;
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(
        &production_sources(false),
    )?)
    .build()
    .await?;
    let uri = "/api/example/route-context/attributes/caf%C3%A9%20%2B%2520?id=query&revision=99&x-example-request-context=query";
    let requests = [
        (Method::GET, Body::empty()),
        (
            Method::POST,
            Body::from(r#"{"id":"body","revision":99,"x-example-request-context":"body"}"#),
        ),
    ];
    for (method, body) in requests {
        let response = app
            .request(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("host", "localhost")
                    .header("x-example-request-context", "header")
                    .header("content-type", "application/json")
                    .body(body)?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = response.json()?;
        assert_eq!(
            json["data"],
            serde_json::json!({"id": "café +%20", "context": "route-middleware", "revision": 7}),
        );
        for header in ["x-example-route-before", "x-example-route-after"] {
            assert_eq!(
                response.headers()[header],
                "example.route-context.attributes"
            );
        }
    }
    for (method, uri, status) in [
        (Method::PUT, uri, StatusCode::METHOD_NOT_ALLOWED),
        (
            Method::GET,
            "/api/example/route-context/attributes",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let response = app
            .request(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(status)?;
        assert!(!response.headers().contains_key("x-example-route-before"));
        assert!(!response.headers().contains_key("x-example-route-after"));
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

// 只装通用 HTTP Provider 的宿主不会被框架强加应用信息 API。
#[tokio::test]
async fn generic_http_host_has_no_application_information_route()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(openrust::cache::CacheServiceProvider::new());
    let source = figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "cache": {"default":"fixture","stores":{"fixture":{"driver":"memory"}}}
        }),
    ));
    let app = openrust::Application::from_provider_runtime(builder.start(source).await?)?;
    let router = openrust::foundation::http::build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/api")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let health = router
        .oneshot(
            Request::get("/health")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(health.status(), StatusCode::OK);
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}

/// 检出唯一模块的数字字符串转换偏离 `PHP` 指数规则的情况。
#[tokio::test]
async fn production_input_numeric_strings_share_php_exponent_cap()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::testing::TestApp;
    use std::time::Duration;

    let mut app = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(production_sources(false)))?,
    )
    .build()
    .await?;
    let outcome: Result<(), Box<dyn std::error::Error>> = async {
        for (text, amount, quantity, bounded) in [
            (
                format!("0.{}1e20000", "0".repeat(19_999)),
                0.1_f64,
                0_i64,
                1.0_f64,
            ),
            (
                format!("1{}e-20000", "0".repeat(20_000)),
                10.0_f64,
                10_i64,
                10.0_f64,
            ),
        ] {
            let response = app
                .request(
                    Request::post("/api/example/input?page=1&label=numeric")
                        .header("host", "localhost")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(
                            &serde_json::json!({"typed":{"quantity":text,"amount":text}}),
                        )?))?,
                )
                .await?;
            response.assert_status(StatusCode::OK)?;
            let output: serde_json::Value = serde_json::from_slice(response.body())?;
            let typed = &output["data"]["typed"];
            assert_eq!(
                typed["amount"].as_f64().map(f64::to_bits),
                Some(amount.to_bits())
            );
            assert_eq!(typed["quantity"].as_i64(), Some(quantity));
            assert_eq!(
                typed["bounded_quantity"].as_f64().map(f64::to_bits),
                Some(bounded.to_bits())
            );
        }
        Ok(())
    }
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    cleanup?;
    Ok(())
}

#[cfg(feature = "hashing")]
#[tokio::test]
async fn module_secret_file_command_uses_same_configured_manager()
-> Result<(), Box<dyn std::error::Error>> {
    use futures_util::FutureExt as _;

    let source = enabled_example_sources(&production_sources(true))?.merge_ordered_entries([
        ("hashing", serde_json::json!({"driver":"argon2id","argon":{"memory":1024,"time":1,"threads":1,"verify":true}})),
    ])?;
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    // 对公开合成数据执行已登记的真实命令；不读取部署秘密或替换生产Hasher。
    let outcome =
        std::panic::AssertUnwindSafe(async {
            let result: Result<(), Box<dyn std::error::Error>> = async {
        let file = tempfile::NamedTempFile::new()?;
        tokio::fs::write(file.path(), [0xff, 0, b'a', 0xfe]).await?;
        app.app()?
            .run_command(
                "example",
                vec![
                    "secret".into(),
                    "hash".into(),
                    file.path().to_str().ok_or("fixture路径不是UTF8")?.into(),
                ],
            )
            .await?;
        let missing_args = app
            .app()?
            .run_command("example secret hash", Vec::new())
            .await;
        if !matches!(missing_args, Err(error) if error.code() == "example_secret_file_argument") {
            return Err("秘密文件命令缺参数没有按公开边界拒绝".into());
        }
        Ok(())
        }.await;
            result
        })
        .catch_unwind()
        .await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    match outcome {
        Ok(result) => {
            if result.is_err()
                && let Err(error) = &shutdown
            {
                eprintln!("示例秘密文件测试关闭失败 code={}", error.code());
            }
            result?;
            shutdown?;
            Ok(())
        }
        Err(payload) => {
            if let Err(error) = shutdown {
                eprintln!("示例秘密文件测试panic后关闭失败 code={}", error.code());
            }
            std::panic::resume_unwind(payload)
        }
    }
}

// 子进程在同一测试可执行文件里调用已登记的真实命令，隔离stdout而不使用unsafe改FD。
#[cfg(feature = "hashing")]
#[tokio::test]
#[ignore = "仅由具名stdout见证父测试携公开fixture环境执行"]
async fn module_secret_file_stdout_child_probe() -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::FutureExt as _;
    let file = std::env::var_os("OPENRUST_PUBLIC_SECRET_STDOUT_FIXTURE")
        .ok_or("缺少公开字节fixture路径")?;
    let reject_file = std::env::var_os("OPENRUST_PUBLIC_SECRET_STDOUT_REJECT_FIXTURE")
        .ok_or("缺少公开拒绝fixture路径")?;
    let source = enabled_example_sources(&production_sources(true))?.merge_ordered_entries([
        ("hashing", serde_json::json!({"driver":"argon2id","argon":{"memory":1024,"time":1,"threads":1,"verify":true}})),
    ])?;
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    let outcome = std::panic::AssertUnwindSafe(async {
        let file = file.into_string().map_err(|_| "fixture路径不是UTF8")?;
        openrust::console::print("OPENRUST_PUBLIC_HASH_OUTPUT_BEGIN");
        app.app()?
            .run_command("example secret hash", vec![file])
            .await?;
        openrust::console::print("OPENRUST_PUBLIC_HASH_OUTPUT_END");
        app.app()?
            .config_repository()
            .set("hashing.driver", serde_json::json!("bcrypt"))?;
        let reject_file = reject_file
            .into_string()
            .map_err(|_| "拒绝fixture路径不是UTF8")?;
        openrust::console::print("OPENRUST_PUBLIC_HASH_REJECT_BEGIN");
        let refused = app
            .app()?
            .run_command("example secret hash", vec![reject_file])
            .await;
        openrust::console::print("OPENRUST_PUBLIC_HASH_REJECT_END");
        if !matches!(refused, Err(error) if error.code() == "example_secret_input_ambiguous") {
            return Err("命令拒绝没有保留输入歧义错误码".into());
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .catch_unwind()
    .await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    match outcome {
        Ok(result) => {
            if result.is_err()
                && let Err(error) = &shutdown
            {
                eprintln!("哈希stdout子测试关闭失败 code={}", error.code());
            }
            result?;
            shutdown?;
            Ok(())
        }
        Err(payload) => {
            if let Err(error) = shutdown {
                eprintln!("哈希stdout子测试panic后关闭失败 code={}", error.code());
            }
            std::panic::resume_unwind(payload)
        }
    }
}

#[cfg(feature = "hashing")]
#[tokio::test]
async fn module_secret_file_stdout_is_one_real_verifiable_hash()
-> Result<(), Box<dyn std::error::Error>> {
    let file = tempfile::NamedTempFile::new()?;
    let reject_file = tempfile::NamedTempFile::new()?;
    tokio::fs::write(reject_file.path(), [b'a'; 73]).await?;
    let bytes = [0xff, 0, b'a', 0xfe];
    tokio::fs::write(file.path(), bytes).await?;
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args([
            "--ignored",
            "--exact",
            "modules::example::tests::module_secret_file_stdout_child_probe",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("OPENRUST_PUBLIC_SECRET_STDOUT_FIXTURE", file.path())
        .env(
            "OPENRUST_PUBLIC_SECRET_STDOUT_REJECT_FIXTURE",
            reject_file.path(),
        )
        .kill_on_drop(true);
    let output =
        tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await??;
    if !output.status.success() {
        return Err("公开哈希stdout子测试未通过".into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    let begin = "OPENRUST_PUBLIC_HASH_OUTPUT_BEGIN";
    let end = "OPENRUST_PUBLIC_HASH_OUTPUT_END";
    if stdout.matches(begin).count() != 1 || stdout.matches(end).count() != 1 {
        return Err("哈希输出边界缺失或重复".into());
    }
    // 逐字消费marker本身的换行；摘要行不裁剪空白或吞掉额外空行。
    let framed = stdout
        .split_once(&format!("{begin}\n"))
        .ok_or("开始marker换行错误")?
        .1
        .split_once(&format!("{end}\n"))
        .ok_or("结束marker换行错误")?
        .0;
    let payload = framed.strip_suffix('\n').ok_or("摘要缺少唯一行尾")?;
    if payload.is_empty()
        || payload.contains(['\n', '\r'])
        || payload != payload.trim()
        || !payload.starts_with("$argon2id$")
    {
        return Err("命令必须逐字只输出一条已编码哈希和一个换行".into());
    }
    let reject_begin = "OPENRUST_PUBLIC_HASH_REJECT_BEGIN\n";
    let reject_end = "OPENRUST_PUBLIC_HASH_REJECT_END\n";
    if stdout.matches(reject_begin).count() != 1 || stdout.matches(reject_end).count() != 1 {
        return Err("拒绝窗口marker缺失或重复".into());
    }
    let rejected = stdout
        .split_once(reject_begin)
        .ok_or("缺少拒绝开始marker")?
        .1
        .split_once(reject_end)
        .ok_or("缺少拒绝结束marker")?
        .0;
    if !rejected.is_empty() {
        return Err("命令拒绝时stdout必须为空".into());
    }
    let hashing = openrust::hashing::HashManager::new(openrust::hashing::HashingConfig {
        driver: "argon2id".into(),
        argon: openrust::hashing::ArgonConfig {
            memory: 1024,
            time: 1,
            threads: 1,
            verify: true,
        },
        ..Default::default()
    })?;
    if !hashing
        .check(bytes, Some(payload), &openrust::hashing::HashOptions::new())
        .await?
    {
        return Err("命令实际输出不能验证原始字节".into());
    }
    if hashing
        .check(
            [0xff, b'a', 0xfe],
            Some(payload),
            &openrust::hashing::HashOptions::new(),
        )
        .await?
    {
        return Err("命令输出丢失NUL身份".into());
    }
    Ok(())
}

#[cfg(feature = "hashing")]
mod nested_manager_secret_command {
    use super::*;
    use futures_util::FutureExt as _;
    use openrust::Result as FrameworkResult;
    use openrust::config::repository::ConfigRepository;
    use openrust::foundation::Application;
    use openrust::hashing::{
        Argon2IdHasher, ArgonConfig, HashInfo, HashManager, HashOptions, Hasher,
    };
    use openrust::support::BoxFuture;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    const OUTPUT_BEGIN: &str = "OPENRUST_NESTED_HASH_OUTPUT_BEGIN";
    const OUTPUT_END: &str = "OPENRUST_NESTED_HASH_OUTPUT_END";
    const REJECT_BEGIN: &str = "OPENRUST_NESTED_HASH_REJECT_BEGIN";
    const REJECT_END: &str = "OPENRUST_NESTED_HASH_REJECT_END";

    #[derive(Default)]
    struct Calls {
        capability: AtomicUsize,
        make: AtomicUsize,
    }
    // 两对象都是普通custom叶，不实现新增manager hook；真实KDF仍由严格Argon完成。
    struct Leaf {
        argon: Argon2IdHasher,
        switch: Option<ConfigRepository>,
        prefix_72: bool,
        calls: Arc<Calls>,
    }
    impl Leaf {
        fn input<'a>(&self, value: &'a [u8]) -> &'a [u8] {
            if self.prefix_72 {
                &value[..value.len().min(72)]
            } else {
                value
            }
        }
    }
    impl Hasher for Leaf {
        fn accepts_exact_input(&self, value: &[u8]) -> FrameworkResult<bool> {
            self.calls.capability.fetch_add(1, Ordering::SeqCst);
            let accepted =
                self.argon.accepts_exact_input(value)? && (!self.prefix_72 || value.len() <= 72);
            if let Some(repository) = &self.switch {
                repository.set("hashing.driver", serde_json::json!("prefix-b"))?;
            }
            Ok(accepted)
        }
        fn info(&self, encoded: &str) -> FrameworkResult<HashInfo> {
            self.argon.info(encoded)
        }
        fn make<'a>(
            &'a self,
            value: &'a [u8],
            options: &'a HashOptions,
        ) -> BoxFuture<'a, FrameworkResult<String>> {
            Box::pin(async move {
                self.calls.make.fetch_add(1, Ordering::SeqCst);
                self.argon.make(self.input(value), options).await
            })
        }
        fn check<'a>(
            &'a self,
            value: &'a [u8],
            encoded: Option<&'a str>,
            options: &'a HashOptions,
        ) -> BoxFuture<'a, FrameworkResult<bool>> {
            self.argon.check(self.input(value), encoded, options)
        }
        fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> FrameworkResult<bool> {
            self.argon.needs_rehash(encoded, options)
        }
        fn verify_configuration(&self, encoded: &str) -> FrameworkResult<bool> {
            self.argon.verify_configuration(encoded)
        }
    }
    fn strict_argon() -> FrameworkResult<Argon2IdHasher> {
        // 沿用已有真实测试成本；不调低生产默认或既有测试下限。
        Argon2IdHasher::new(ArgonConfig {
            memory: 1024,
            time: 1,
            threads: 1,
            verify: true,
        })
    }
    struct Fixture {
        inner_repository: ConfigRepository,
        whole: Arc<Calls>,
        prefix: Arc<Calls>,
    }
    impl Fixture {
        fn counts(&self) -> [usize; 4] {
            // A能力/A真实make委派/B能力/B真实make委派。
            [
                self.whole.capability.load(Ordering::SeqCst),
                self.whole.make.load(Ordering::SeqCst),
                self.prefix.capability.load(Ordering::SeqCst),
                self.prefix.make.load(Ordering::SeqCst),
            ]
        }
    }
    async fn install(app: &Application) -> FrameworkResult<Fixture> {
        let inner_repository = ConfigRepository::new(figment::Figment::from(
            figment::providers::Serialized::defaults(
                serde_json::json!({"hashing":{"driver":"whole-a"}}),
            ),
        ));
        let inner = Arc::new(HashManager::from_repository(inner_repository.clone(), 2)?);
        let whole = Arc::new(Calls::default());
        let prefix = Arc::new(Calls::default());
        let argon = strict_argon()?;
        let a = Arc::new(Leaf {
            argon: argon.clone(),
            switch: Some(inner_repository.clone()),
            prefix_72: false,
            calls: Arc::clone(&whole),
        });
        let b = Arc::new(Leaf {
            argon,
            switch: None,
            prefix_72: true,
            calls: Arc::clone(&prefix),
        });
        inner.extend("whole-a", move || Ok(Arc::clone(&a) as Arc<dyn Hasher>))?;
        inner.extend("prefix-b", move || Ok(Arc::clone(&b) as Arc<dyn Hasher>))?;
        let outer = app.state().resolve::<HashManager>().await?;
        outer.extend("cli-inner", move || {
            Ok(Arc::clone(&inner) as Arc<dyn Hasher>)
        })?;
        app.config_repository()
            .set("hashing.driver", serde_json::json!("cli-inner"))?;
        Ok(Fixture {
            inner_repository,
            whole,
            prefix,
        })
    }

    #[tokio::test]
    #[ignore = "仅由具名嵌套stdout见证父测试携公开fixture环境执行"]
    async fn nested_manager_stdout_child_probe() -> Result<(), Box<dyn std::error::Error>> {
        let file = std::env::var_os("OPENRUST_PUBLIC_NESTED_HASH_FIXTURE")
            .ok_or("缺少公开73字节fixture路径")?
            .into_string()
            .map_err(|_| "fixture路径不是UTF8")?;
        let source = enabled_example_sources(&production_sources(true))?.merge_ordered_entries([
            ("hashing", serde_json::json!({"driver":"argon2id","argon":{"memory":1024,"time":1,"threads":1,"verify":true}})),
        ])?;
        let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        let outcome = std::panic::AssertUnwindSafe(async {
            let fixture = install(app.app()?).await?;
            assert_eq!(fixture.counts(), [0, 0, 0, 0]);
            openrust::console::print(OUTPUT_BEGIN);
            app.app()?
                .run_command("example secret hash", vec![file.clone()])
                .await?;
            openrust::console::print(OUTPUT_END);
            assert_eq!(
                fixture.inner_repository.get("hashing.driver")?,
                Some(serde_json::json!("prefix-b"))
            );
            assert_eq!(fixture.counts(), [1, 1, 0, 0]);
            let before = fixture.counts();
            openrust::console::print(REJECT_BEGIN);
            let refused = app
                .app()?
                .run_command("example secret hash", vec![file])
                .await;
            openrust::console::print(REJECT_END);
            assert!(
                matches!(refused, Err(error) if error.code() == "example_secret_input_ambiguous")
            );
            assert_eq!(fixture.counts(), [1, 1, 1, 0]);
            assert_eq!(fixture.whole.make.load(Ordering::SeqCst), before[1]);
            assert_eq!(fixture.prefix.make.load(Ordering::SeqCst), before[3]);
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .catch_unwind()
        .await;
        let shutdown = app
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await;
        match outcome {
            Ok(result) => {
                if result.is_err()
                    && let Err(error) = &shutdown
                {
                    eprintln!("嵌套哈希子测试关闭失败 code={}", error.code());
                }
                result?;
                shutdown?;
                Ok(())
            }
            Err(payload) => {
                if let Err(error) = shutdown {
                    eprintln!("嵌套哈希子测试panic后关闭失败 code={}", error.code());
                }
                std::panic::resume_unwind(payload)
            }
        }
    }
    fn window<'a>(
        stdout: &'a str,
        begin: &str,
        end: &str,
    ) -> Result<&'a str, Box<dyn std::error::Error>> {
        let begin = format!("{begin}\n");
        let end = format!("{end}\n");
        if stdout.matches(&begin).count() != 1 || stdout.matches(&end).count() != 1 {
            return Err("嵌套命令输出marker缺失或重复".into());
        }
        Ok(stdout
            .split_once(&begin)
            .ok_or("缺少开始marker")?
            .1
            .split_once(&end)
            .ok_or("缺少结束marker")?
            .0)
    }
    #[tokio::test]
    async fn actual_nested_command_keeps_complete_input_and_rejects_next_without_output_or_kdf()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        let bytes = [b'a'; 73];
        tokio::fs::write(file.path(), bytes).await?;
        let module = module_path!()
            .split_once("::")
            .map_or(module_path!(), |(_, module)| module);
        let name = format!("{module}::nested_manager_stdout_child_probe");
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command
            .args([
                "--ignored",
                "--exact",
                &name,
                "--nocapture",
                "--test-threads=1",
            ])
            .env("OPENRUST_PUBLIC_NESTED_HASH_FIXTURE", file.path())
            .kill_on_drop(true);
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await??;
        assert!(
            output.status.success(),
            "真实嵌套命令子进程失败：{}",
            output.status
        );
        let stdout = String::from_utf8(output.stdout)?;
        assert!(stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"));
        let framed = window(&stdout, OUTPUT_BEGIN, OUTPUT_END)?;
        let encoded = framed.strip_suffix('\n').ok_or("哈希缺少唯一换行")?;
        if encoded.is_empty()
            || encoded.contains(['\n', '\r'])
            || encoded != encoded.trim()
            || !encoded.starts_with("$argon2id$")
        {
            return Err("真实命令必须仅输出一条Argon编码哈希和一个换行".into());
        }
        assert_eq!(window(&stdout, REJECT_BEGIN, REJECT_END)?, "");
        let verifier = strict_argon()?;
        assert!(
            verifier
                .check(&bytes, Some(encoded), &HashOptions::new())
                .await?
        );
        assert!(
            !verifier
                .check(&bytes[..72], Some(encoded), &HashOptions::new())
                .await?
        );
        Ok(())
    }
}

// 数值日期通过现有公开模块请求管线，不改 Request.date 默认或输入中间件。
mod numeric_date_http {
    use super::{enabled_example_sources, production_sources};
    use futures_util::FutureExt as _;
    use openrust::http::axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use openrust::testing::TestApp;
    use serde_json::{Value, json};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
    type App = TestApp<crate::bootstrap::Server>;

    async fn post(app: &mut App, mut typed: Value) -> TestResult<(StatusCode, Value)> {
        typed["title"] = json!("  Numeric  ");
        let response = app
            .request(
                Request::post("/api/example/input")
                    .header("host", "localhost")
                    .header("accept", "application/json")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"title":"  normalized  ", "typed":typed}).to_string(),
                    ))?,
            )
            .await?;
        Ok((response.status(), serde_json::from_slice(response.body())?))
    }

    async fn success_cases(app: &mut App) -> TestResult {
        for (timestamp, format, seconds, micros, expected) in [
            (json!(-1.25), -1.25, -1, 250_000, "-1.250000"),
            (json!(-0.25), -0.25, 0, 250_000, "-0.250000"),
            (json!(0.000_000_5), 0.000_000_5, 0, 1, "0.000001"),
            (json!(-0.000_000_5), -0.000_000_5, 0, 1, "-0.000001"),
            (json!(-0.000_000_49), -0.000_000_49, 0, 0, "0.000000"),
            (json!(" \t+1.25e0\r\n"), 1.25, 1, 250_000, "1.250000"),
            (json!(" \0-1.25\0 "), -1.25, -1, 250_000, "-1.250000"),
        ] {
            let (status, body) = post(
                app,
                json!({"numeric_timestamp":timestamp,"numeric_format":format}),
            )
            .await?;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["code"], "ok");
            assert_eq!(body["data"]["title"], "normalized");
            assert_eq!(body["data"]["typed"]["title"], "Numeric");
            assert_eq!(
                body["data"]["typed"]["numeric_timestamp"],
                json!({"epoch_seconds":seconds,"microseconds":micros})
            );
            assert_eq!(body["data"]["typed"]["numeric_format"], expected);
        }
        let (status, body) = post(app, json!({})).await?;
        assert_eq!(status, StatusCode::OK);
        let typed = body["data"]["typed"]
            .as_object()
            .ok_or("typed响应缺少对象")?;
        assert!(!typed.contains_key("numeric_timestamp"));
        assert!(!typed.contains_key("numeric_format"));
        Ok(())
    }

    fn invalid_timestamp_cases() -> Vec<(Value, &'static str)> {
        vec![
            (json!(true), "input_date_type_invalid"),
            (Value::Null, "input_date_type_invalid"),
            (json!(""), "input_date_type_invalid"),
            (json!(" \t "), "input_date_type_invalid"),
            (json!([]), "input_date_type_invalid"),
            (json!({}), "input_date_type_invalid"),
            (
                json!("1\0.25numeric-private-sentinel"),
                "input_date_format_invalid",
            ),
            (
                json!("Y-m-d numeric-private-sentinel"),
                "input_date_format_invalid",
            ),
            (json!(i64::MAX), "input_calendar_out_of_range"),
            (json!("9223372036854775807"), "input_calendar_out_of_range"),
        ]
    }
    async fn invalid_cases(app: &mut App) -> TestResult {
        for (value, code) in invalid_timestamp_cases() {
            let (status, body) = post(app, json!({"numeric_timestamp":value})).await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["code"], code);
            assert!(!body.to_string().contains("numeric-private-sentinel"));
            assert!(!body.to_string().contains("9223372036854775807"));
            assert!(body["data"].is_null());
        }
        for value in [
            json!("-1.25"),
            json!("Y-m-d numeric-private-sentinel"),
            json!(false),
            Value::Null,
        ] {
            let (status, body) = post(app, json!({"numeric_format":value})).await?;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["code"], "example_numeric_format_type");
            assert!(!body.to_string().contains("numeric-private-sentinel"));
            assert!(body["data"].is_null());
        }
        Ok(())
    }

    #[tokio::test]
    async fn production_input_numeric_dates_use_public_core_and_preserve_normalization()
    -> TestResult {
        let mut app = App::builder(enabled_example_sources(&production_sources(true))?)
            .build()
            .await?;
        let outcome = std::panic::AssertUnwindSafe(async {
            success_cases(&mut app).await?;
            invalid_cases(&mut app).await
        })
        .catch_unwind()
        .await;
        let shutdown = app
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await;
        match outcome {
            Ok(result) => {
                if result.is_err()
                    && let Err(error) = &shutdown
                {
                    eprintln!("数值日期HTTP测试关闭失败 code={}", error.code());
                }
                result?;
                shutdown?;
                Ok(())
            }
            Err(payload) => {
                if let Err(error) = shutdown {
                    eprintln!("数值日期HTTP测试panic后关闭失败 code={}", error.code());
                }
                std::panic::resume_unwind(payload)
            }
        }
    }
}
