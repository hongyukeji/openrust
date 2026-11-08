//! 使用与生产相同的 Provider 清单测试应用 HTTP 路由。
{{seed_module}}#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
#[path = "support/message_bags.rs"]
mod message_bags;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_cookie.rs"]
mod request_cookie;
#[path = "support/request_input.rs"]
mod request_input;
#[path = "support/request_redirect.rs"]
mod request_redirect;
#[path = "support/request_session.rs"]
mod request_session;
#[path = "../routes/mod.rs"]
mod routes;

use app::http::controllers::home::{
    HomeConstruction, HomeConstructionInput, HomePresenter, HomeResponse,
};
use bootstrap::OpenRustGeneratedApplication as GeneratedApplication;

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::cache::{Cache, CacheStore};
use openrust::config::ConfigSources;
use openrust::container::attributes::{
    Cache as CacheAttribute, Config, Context, Give, Log, RequestAttribute, RouteParameter,
};
use openrust::container::{
    AttributeValue, ClassAttribute, ClassDescriptor, ConstructorParam, ParameterKey,
    ParameterOverrides, ParameterValue, ServiceKey,
};
use openrust::foundation::RegisterContext;
use openrust::logging::{LogLevel, Logger, context::Repository as LogContext};
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::TestApp;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn test_configuration() -> openrust::config::FrameworkConfig {
    let mut config = openrust::config::FrameworkConfig::default();
    config.app.env = openrust::config::Environment::Testing;
    config
}

struct GeneratedRequestValues {
    id: Arc<String>,
    source: Arc<String>,
}

struct RequestAttributesProvider;

impl ServiceProvider for RequestAttributesProvider {
    fn boot<'a>(&'a self, app: &'a mut openrust::foundation::BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        use openrust::http::axum::{extract::Request, middleware::{Next, from_fn}, response::Response};
        use openrust::http::{Inject, Json, RequestInfo};
        Box::pin(async move {
            async fn attach(info: RequestInfo, request: Request, next: Next) -> openrust::Result<Response> {
                info.attributes().insert("source", ParameterValue::new("服务端".to_owned()))?;
                Ok(next.run(request).await)
            }
            async fn show(Inject(values): Inject<GeneratedRequestValues>) -> Json<serde_json::Value> {
                Json(serde_json::json!({"id": &*values.id, "source": &*values.source}))
            }
            app.routes(|routes| {
                routes.group(openrust::routing::RouteGroup::new().middleware(from_fn(attach)), |routes| {
                    routes.get("/test-request-attributes/{id}", show)
                })
            })?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn generated_application_injects_request_attributes() -> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(test_configuration())).merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let mut builder = TestApp::<GeneratedApplication>::builder(source);
    builder.configure_providers(|builder| {
        builder.provider(RequestAttributesProvider);
        builder.class_descriptors([ClassDescriptor::concrete::<GeneratedRequestValues>(
            "App/GeneratedRequestValues",
            vec![
                ConstructorParam::scalar::<String>("id")
                    .with_attribute(ClassAttribute::contextual(|| Ok(RouteParameter::default()))),
                ConstructorParam::scalar::<String>("source")
                    .with_attribute(ClassAttribute::contextual(|| Ok(RequestAttribute::new("source")))),
            ],
            |args| Ok(Arc::new(GeneratedRequestValues {
                id: args.value(0)?.downcast()?,
                source: args.value(1)?.downcast()?,
            })),
        )]);
    });
    let mut app = builder.build().await?;
    let response = app.request(Request::get("/test-request-attributes/%E6%96%87?source=external")
        .header("source", "external")
        .body(Body::empty())?).await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], serde_json::json!({"id": "文", "source": "服务端"}));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)).await?;
    Ok(())
}

struct ScopedCache(Arc<CacheStore>);

struct NamedLogger(Arc<Logger>, Arc<String>);

#[tokio::test]
async fn generated_application_injects_named_logger() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("preview.log");
    let source = Figment::from(Serialized::defaults(test_configuration()))
        .merge(Serialized::defaults(serde_json::json!({
            "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
            "logging": {"channels": {"preview": {"driver": "single", "path": path}}},
            "view": {"directory": "resources/views"}
        })));
    let configuration: bootstrap::Configuration = source.extract()?;
    let mut builder =
        openrust::foundation::ProviderBuilder::for_application::<GeneratedApplication>(&configuration);
    builder.class_descriptors([ClassDescriptor::concrete::<NamedLogger>(
        "App/NamedLogger",
        vec![
            ConstructorParam::class::<Logger>("logger").with_attribute(
                ClassAttribute::contextual(|| Ok(Log::new("preview").named("generated.preview"))),
            ),
            ConstructorParam::scalar::<String>("entry").with_attribute(
                ClassAttribute::contextual(|| Ok(Context::<String>::new("entry.name"))),
            ),
        ],
        |args| Ok(Arc::new(NamedLogger(args.service(0)?, args.value(1)?.downcast()?))),
    )]);
    let app = openrust::Application::from_provider_runtime(builder.start(source).await?)?;
    let context = app.state().resolve::<LogContext>().await?;
    context.add("entry.name", serde_json::json!("generated"));
    context.add_hidden("private", serde_json::json!("example-secret"));
    let logger = app.state().resolve::<NamedLogger>().await?;
    assert_eq!(logger.1.as_str(), "generated");
    logger.0.log(LogLevel::Notice, "生成项目日志", serde_json::Map::new()).await?;
    let log = std::fs::read_to_string(path)?;
    assert!(log.contains("generated.preview.NOTICE: 生成项目日志 {\"entry.name\":\"generated\"}"));
    assert!(!log.contains("example-secret"));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)).await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_injects_scoped_cache_store() -> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(test_configuration())).merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[cache.stores.preview]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let configuration: bootstrap::Configuration = source.extract()?;
    let mut builder =
        openrust::foundation::ProviderBuilder::for_application::<GeneratedApplication>(&configuration);
    builder.class_descriptors([ClassDescriptor::concrete::<ScopedCache>(
        "App/ScopedCache",
        vec![
            ConstructorParam::class::<CacheStore>("cache").with_attribute(
                ClassAttribute::contextual(|| Ok(CacheAttribute::new("preview").memoized())),
            ),
        ],
        |args| Ok(Arc::new(ScopedCache(args.service(0)?))),
    )]);
    let app = openrust::Application::from_provider_runtime(builder.start(source).await?)?;
    let container = app.state().container();
    let cache = container.resolve::<Cache>().await?;
    let preview = cache.store("preview").await?;
    let scope = container.scope()?;
    let handle = scope.resolve::<ScopedCache>().await?;
    assert_eq!(handle.0.get::<i32>("title").await?, None);
    preview.set("title", &17, None).await?;
    assert_eq!(handle.0.get::<i32>("title").await?, None);
    assert_eq!(cache.default_store().await?.get::<i32>("title").await?, None);
    let other_scope = container.scope()?;
    let other = other_scope.resolve::<ScopedCache>().await?;
    assert_eq!(other.0.get::<i32>("title").await?, Some(17));
    handle.0.set("title", &18, None).await?;
    assert_eq!(handle.0.get::<i32>("title").await?, Some(18));
    assert_eq!(preview.get::<i32>("title").await?, Some(18));
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)).await?;
    Ok(())
}

// 生成应用在公开 server 配置上按精确环境启用主机信任列表。
#[tokio::test]
async fn generated_application_applies_trusted_hosts() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = test_configuration();
    config.app.env = openrust::config::Environment::Development;
    config.server.trusted_hosts = Some(openrust::config::TrustedHostsConfig::default());
    let source = Figment::from(Serialized::defaults(config)).merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let mut app = TestApp::<GeneratedApplication>::builder(source)
        .build()
        .await?;
    app.request(
        Request::get("/api")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    app.request(
        Request::get("/api")
            .header("host", "attacker.test")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::BAD_REQUEST)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

struct RuntimeGreeting(&'static str);

struct RuntimeGreetingProvider(&'static str);

impl ServiceProvider for RuntimeGreetingProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(RuntimeGreeting(self.0)))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn generated_application_exposes_runtime_provider_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(test_configuration()))
        .merge(Toml::string("[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n"));
    let configuration: bootstrap::Configuration = source.extract()?;
    let builder =
        openrust::foundation::ProviderBuilder::for_application::<GeneratedApplication>(&configuration);
    let mut app = openrust::Application::from_provider_runtime(builder.start(source).await?)?;
    assert_eq!(app.environment().as_str(), "testing");
    assert!(app.running_unit_tests());
    assert!(!app.is_production());
    let first = app
        .register_provider(RuntimeGreetingProvider("初次登记"))
        .await?;
    let old = app.state().resolve::<RuntimeGreeting>().await?;
    assert_eq!(old.0, "初次登记");
    assert_eq!(
        app.register_provider(RuntimeGreetingProvider("忽略重复"))
            .await?,
        first
    );
    assert_eq!(
        app.state().resolve::<RuntimeGreeting>().await?.0,
        "初次登记"
    );
    assert_eq!(
        app.force_register_provider(RuntimeGreetingProvider("强制更新"))
            .await?,
        first
    );
    assert_eq!(old.0, "初次登记");
    assert_eq!(
        app.state().resolve::<RuntimeGreeting>().await?.0,
        "强制更新"
    );
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_serves_api_and_health() -> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[app]\nname = 'configured-application'\n[server]\nroot_target = 'view:index.html'\n[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let root: serde_json::Value = source.extract()?;
    let mut entries = root
        .as_object()
        .ok_or("生成配置根必须是对象")?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    entries.push((
        "template_order".to_owned(),
        serde_json::json!({"z_label": "先声明", "a_label": "后声明"}),
    ));
    let sources = ConfigSources::from_ordered_entries(entries)?;
    let mut app = TestApp::<GeneratedApplication>::builder(sources)
        .build()
        .await?;
    let state = app.app()?.state().clone();
    assert!(!state.container().contains::<HomePresenter>());
    assert!(state.container().contains::<dyn HomeResponse>());
    let symbol = ServiceKey::named(HomePresenter::SYMBOL);
    assert!(!state.container().resolved_key(&symbol)?);
    let resolved_config = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolved_config);
    state
        .container()
        .after_resolving_attribute::<Config<String>>(move |_, _| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        })?;
    let resolved_give = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolved_give);
    state
        .container()
        .after_resolving_attribute::<Give<HomeConstruction>>(move |event, _| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                let construction = event.service::<HomeConstruction>()?.ok_or_else(|| {
                    openrust::Error::custom(
                        "home_construction_view_missing",
                        "Give 回调缺少构造对象",
                    )
                })?;
                construction.verify()?;
                seen.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        })?;
    let resolved_input = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolved_input);
    state
        .container()
        .after_resolving_attribute::<HomeConstructionInput>(move |event, _| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                assert!(matches!(event.value(), AttributeValue::One(value) if *value.downcast::<bool>()?));
                seen.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        })?;
    let resolved_construction = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolved_construction);
    state.container().resolving(
        Some(ServiceKey::typed::<HomeConstruction>()),
        move |event, _| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                if let Some(construction) = event.value::<HomeConstruction>()? {
                    construction.verify()?;
                    seen.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            })
        },
    )?;
    let resolved_home = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolved_home);
    state.container().after_resolving(
        Some(ServiceKey::typed::<HomePresenter>()),
        move |event, _| {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                if event.key() == &ServiceKey::typed::<dyn HomeResponse>() {
                    assert!(event.value::<HomePresenter>()?.is_some());
                    assert!(event.value::<dyn HomeResponse>()?.is_some());
                    seen.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            })
        },
    )?;
    let keys = app.app()?.config_repository().all()?["template_order"]
        .as_object()
        .ok_or("有序测试区段必须是对象")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(keys, ["z_label", "a_label"]);
    let request = Request::builder().uri("/api").body(Body::empty())?;
    app.request(request)
        .await?
        .assert_status(StatusCode::OK)?
        .assert_json(&serde_json::json!({
            "success": true,
            "code": "ok",
            "message": "ok",
            "data": {"name": "configured-application"}
        }))?;
    assert!(!state.container().resolved_key(&symbol)?);
    assert_eq!(resolved_home.load(Ordering::Relaxed), 1);
    assert_eq!(resolved_input.load(Ordering::Relaxed), 1);
    assert_eq!(resolved_construction.load(Ordering::Relaxed), 1);
    assert_eq!(resolved_give.load(Ordering::Relaxed), 1);
    assert_eq!(resolved_config.load(Ordering::Relaxed), 0);
    assert_generated_class_factories(&state).await?;
    assert_eq!(resolved_home.load(Ordering::Relaxed), 1);
    let first = state.resolve_key::<dyn HomeResponse>(symbol.clone()).await?;
    let second = state.resolve_key::<dyn HomeResponse>(symbol.clone()).await?;
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(
        first
            .response()
            .data
            .as_ref()
            .and_then(|data| data.get("name"))
            .map(String::as_str),
        Some("configured-application"),
    );
    let startup_name = app.app()?.config().app.name.clone();
    app.app()?
        .config_repository()
        .set("app.name", serde_json::json!("运行期名称"))?;
    app.app()?
        .config_repository()
        .set("template_order.z_label", serde_json::json!("已复核"))?;
    let keys = app.app()?.config_repository().all()?["template_order"]
        .as_object()
        .ok_or("有序测试区段必须是对象")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(keys, ["z_label", "a_label"]);
    assert_eq!(
        app.app()?.section::<serde_json::Value>("app")?["name"],
        "运行期名称"
    );
    assert_eq!(app.app()?.config().app.name, startup_name);
    let live_presenter = state.resolve_key::<dyn HomeResponse>(symbol.clone()).await?;
    assert_eq!(
        live_presenter
            .response()
            .data
            .as_ref()
            .and_then(|data| data.get("name"))
            .map(String::as_str),
        Some("运行期名称"),
    );
    assert_eq!(resolved_config.load(Ordering::Relaxed), 4);
    let api = app
        .request(Request::get("/api").body(Body::empty())?)
        .await?;
    assert!(api.headers().get("set-cookie").is_none());
    api.assert_json(&serde_json::json!({
        "success": true,
        "code": "ok",
        "message": "ok",
        "data": {"name": "configured-application"}
    }))?;
    assert_eq!(resolved_config.load(Ordering::Relaxed), 4);
    assert_parameterized_home_factory_preserves_scope(&app).await?;
    let health = Request::builder().uri("/health").body(Body::empty())?;
    app.request(health).await?.assert_status(StatusCode::OK)?;
    let page = app
        .request(Request::builder().uri("/").body(Body::empty())?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(page.body())?;
    assert!(html.contains("configured-application"));
    assert!(html.contains("<p id=\"default-error\"></p>"));
    assert!(html.contains("<p id=\"other-error\"></p>"));
    let missing = Request::builder().uri("/api/missing").body(Body::empty())?;
    let fallback = app.request(missing).await?;
    fallback.assert_status(StatusCode::NOT_FOUND)?;
    assert_eq!(fallback.body(), "未找到路由：/api/missing".as_bytes());
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

async fn assert_generated_class_factories(
    state: &openrust::foundation::ApplicationState,
) -> Result<(), Box<dyn std::error::Error>> {
    let container = state.container();
    let abstract_key = ServiceKey::typed::<dyn HomeResponse>();
    let concrete_key = ServiceKey::typed::<HomePresenter>();
    let symbol = ServiceKey::named(HomePresenter::SYMBOL);
    let scope = container.scope()?;
    let mut arguments = ParameterOverrides::new();
    arguments.insert(
        ParameterKey::name(HomePresenter::NAME_PARAMETER),
        ParameterValue::new(42_i64),
    );
    let wrapper = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == abstract_key)
        .ok_or("生成应用缺少异键类工厂")?;
    let redirected = wrapper
        .concrete
        .invoke_with::<dyn HomeResponse>(scope.factory_resolver()?, arguments.clone())
        .await?;
    assert_eq!(
        redirected.response().data.ok_or("缺少应用名称")?["name"],
        "42"
    );
    arguments.insert(
        ParameterKey::name(HomePresenter::NAME_PARAMETER),
        ParameterValue::new(f64::NAN),
    );
    assert!(matches!(
        wrapper.concrete.invoke_with::<dyn HomeResponse>(scope.factory_resolver()?, arguments.clone()).await,
        Err(error) if error.code() == "container_scalar_warning"
    ));
    assert!(!container.resolved_key(&abstract_key)?);
    assert!(!container.resolved_key(&concrete_key)?);

    container
        .bind_class::<HomePresenter>(concrete_key.clone(), symbol)
        .await?;
    let same = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == concrete_key)
        .ok_or("生成应用缺少同键类工厂")?;
    let direct = same
        .concrete
        .invoke_with::<HomePresenter>(scope.factory_resolver()?, arguments)
        .await?;
    assert_eq!(
        direct.response().data.ok_or("缺少应用名称")?["name"],
        "configured-application"
    );
    assert!(!container.resolved_key(&concrete_key)?);
    scope
        .shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

async fn assert_parameterized_home_factory_preserves_scope(
    app: &TestApp<GeneratedApplication>,
) -> Result<(), Box<dyn std::error::Error>> {
    struct HomeRequestScope;

    let state = app.app()?.state();
    let symbol = ServiceKey::named(HomePresenter::SYMBOL);
    // 根缓存与带参构造分离。工厂的普通依赖仍在实际请求作用域构造。
    let scope_constructions = Arc::new(AtomicUsize::new(0));
    let factory_constructions = Arc::clone(&scope_constructions);
    assert!(
        state
            .container()
            .scoped_if::<HomeRequestScope, _>(move |_| {
                factory_constructions.fetch_add(1, Ordering::Relaxed);
                Box::pin(async { Ok(Arc::new(HomeRequestScope)) })
            })
            .await?
    );
    assert!(
        !state
            .container()
            .scoped_if::<HomePresenter, _>(|_| {
                Box::pin(async {
                    Err(openrust::Error::custom(
                        "unexpected_home_factory",
                        "已有工厂被替换",
                    ))
                })
            })
            .await?
    );
    state
        .container()
        .scoped::<HomePresenter, _>(|resolver| {
            Box::pin(async move {
                resolver.resolve::<HomeRequestScope>().await?;
                let name = resolver
                    .parameters()
                    .get(&ParameterKey::name(HomePresenter::NAME_PARAMETER))
                    .map(ParameterValue::downcast::<String>)
                    .transpose()?
                    .unwrap_or_default();
                Ok(Arc::new(HomePresenter::new(name)))
            })
        })
        .await?;
    let typed_root: Arc<dyn HomeResponse> = state.resolve::<HomePresenter>().await?;
    let projected_root = state
        .resolve_key::<dyn HomeResponse>(symbol.clone())
        .await?;
    assert!(Arc::ptr_eq(&typed_root, &projected_root));
    assert_eq!(scope_constructions.load(Ordering::Relaxed), 1);
    for expected_count in [2, 3] {
        let response = app
            .request(Request::get("/api").body(Body::empty())?)
            .await?;
        response.assert_status(StatusCode::OK)?;
        assert!(response.headers().get("set-cookie").is_none());
        let body: serde_json::Value = response.json()?;
        assert_eq!(body["data"]["name"], "configured-application");
        assert_eq!(scope_constructions.load(Ordering::Relaxed), expected_count);
    }
    let after_requests = state.resolve_key::<dyn HomeResponse>(symbol).await?;
    assert!(Arc::ptr_eq(&projected_root, &after_requests));

    let container = state.container();
    let binding = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == ServiceKey::typed::<HomePresenter>())
        .ok_or("生成应用缺少呈现器的原工厂")?;
    assert!(binding.shared);
    let scope = container.scope()?;
    let mut arguments = ParameterOverrides::new();
    let _ = arguments.insert(
        ParameterKey::name(HomePresenter::NAME_PARAMETER),
        ParameterValue::new(String::from("直接调用")),
    );
    let raw = binding
        .concrete
        .invoke_with::<HomePresenter>(scope.factory_resolver()?, arguments)
        .await?;
    assert_eq!(
        raw
            .response()
            .data
            .as_ref()
            .and_then(|data| data.get("name"))
            .map(String::as_str),
        Some("直接调用"),
    );
    assert_eq!(scope_constructions.load(Ordering::Relaxed), 4);
    scope
        .shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_exposes_request_input_to_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_input::assert_request_input::<GeneratedApplication>(source).await
}

#[tokio::test]
async fn generated_application_consumes_message_bags_through_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    message_bags::assert_message_bags::<GeneratedApplication>(source).await
}

/// 复用公开 helper 验证概览路由覆盖/静态顺序、动作跳转、确认回跳及错误闪存。
#[tokio::test]
async fn generated_application_redirects_to_named_route_and_restores_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_redirect::assert_request_redirect::<GeneratedApplication>(source).await
}

#[tokio::test]
async fn generated_application_exposes_cookie_contract() -> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_cookie::assert_request_cookie::<GeneratedApplication>(source, false, |_| Ok(())).await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_exposes_session_lifecycle() -> Result<(), Box<dyn std::error::Error>>
{
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    request_session::assert_request_session::<GeneratedApplication>(source, false).await?;
    Ok(())
}

#[tokio::test]
async fn generated_original_host_supports_explicit_cookie_session_without_database()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    ));
    let source = source.merge(Serialized::defaults(
        serde_json::json!({"session":{"lottery":[1,1]}}),
    ));
    request_session::assert_original_host_cookie_session::<GeneratedApplication>(source).await
}
