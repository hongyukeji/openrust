#[path = "../config/horizon.rs"]
mod configuration;
use openrust::{config::Environment, foundation::configuration::Routing, testing::TestApp};
use openrust_horizon::{Horizon, HorizonServiceProvider};
use serde_json::json;

#[tokio::test]
async fn environment_defaults_and_provider_routes_follow_configuration_without_redis() {
    let mut app =
        TestApp::create(|builder| builder.with_providers(vec![Box::new(HorizonServiceProvider)]))
            .unwrap();
    std::fs::write(app.root().join(".env"), "APP_NAME=My-Shop\nHORIZON_PATH=operations/queue\nHORIZON_DOMAIN=ops.example.test\nHORIZON_PREFIX=shop_queue:\n").unwrap();
    app.boot().await.unwrap();
    let config = app
        .app()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap();
    assert_eq!(config.get("horizon.prefix"), Some(json!("shop_queue:")));
    app.get("/operations/queue")
        .await
        .unwrap()
        .assert_status(http::StatusCode::NOT_FOUND);
    let response = app
        .call(
            http::Request::builder()
                .uri("http://ops.example.test/operations/queue")
                .header("host", "ops.example.test")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.assert_status(http::StatusCode::FORBIDDEN);
    let mut default_app =
        TestApp::create(|builder| builder.with_providers(vec![Box::new(HorizonServiceProvider)]))
            .unwrap();
    default_app.boot().await.unwrap();
    let env = default_app.app().make::<Environment>().await.unwrap();
    let defaults = configuration::load(&env).unwrap();
    assert_eq!(defaults["prefix"], "laravel_horizon:");
    assert_eq!(defaults["middleware"], json!(["web"]));
    assert_eq!(defaults.get("name"), Some(&serde_json::Value::Null));
    assert_eq!(defaults["waits"], json!({"redis:default":60}));
    assert_eq!(defaults["silenced"], json!([]));
    assert_eq!(defaults["silenced_tags"], json!([]));
    assert_eq!(defaults["defaults"]["supervisor-1"]["nice"], 0);
    assert_eq!(config.get("horizon.watch"), Some(defaults["watch"].clone()));
    assert_eq!(
        defaults["watch"],
        json!([
            "app",
            "bootstrap",
            "config",
            "database",
            "public/*.rs",
            "resources/*.rs",
            "resources/views",
            "routes",
            "main.rs",
            "build.rs",
            "Cargo.lock",
            "Cargo.toml",
            ".env"
        ])
    );
}

#[tokio::test]
async fn dashboard_uses_each_applications_configured_name_as_escaped_text() {
    fn routes() -> Result<(), openrust::routing::RoutingError> {
        Horizon::auth(|_| Box::pin(async { Ok(true) })).unwrap();
        Horizon::routes()
    }

    for (name, suffix) in [
        (None, ""),
        (Some("0"), ""),
        (
            Some("运维 & </title><script>alert(1)</script>"),
            " - 运维 &amp; &lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;",
        ),
    ] {
        let mut app = TestApp::create(|builder| {
            builder
                .with_providers(vec![Box::new(HorizonServiceProvider)])
                .with_routing(Routing::default().api(routes))
        })
        .unwrap();
        if let Some(name) = name {
            std::fs::write(app.root().join(".env"), format!("HORIZON_NAME='{name}'\n")).unwrap();
        }
        app.get("/api/horizon")
            .await
            .unwrap()
            .assert_ok()
            .assert_see(format!("<title>Horizon{suffix}</title>"), false)
            .assert_see(format!("<h1>Horizon{suffix}</h1>"), false)
            .assert_dont_see("<script>alert(1)</script>", false);
    }
}

#[tokio::test]
async fn application_name_derives_prefix_and_empty_path_mounts_at_root() {
    let mut app =
        TestApp::create(|builder| builder.with_providers(vec![Box::new(HorizonServiceProvider)]))
            .unwrap();
    std::fs::write(
        app.root().join(".env"),
        "APP_NAME=My-Shop\nHORIZON_PATH=/\n",
    )
    .unwrap();
    app.boot().await.unwrap();
    let config = app
        .app()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap();
    assert_eq!(
        config.get("horizon.prefix"),
        Some(json!("my_shop_horizon:"))
    );
    app.get("/")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN);
    app.get("/api/stats")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN);
}

fn explicit() -> Result<(), openrust::routing::RoutingError> {
    Horizon::routes_at("/custom")?;
    Horizon::routes_at("/ignored-duplicate")
}
#[tokio::test]
async fn explicit_routes_take_precedence_and_automatic_routes_can_be_disabled() {
    let mut explicit = TestApp::create(|b| {
        b.with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(explicit))
    })
    .unwrap();
    explicit
        .get("/api/custom")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN);
    explicit
        .get("/horizon")
        .await
        .unwrap()
        .assert_status(http::StatusCode::NOT_FOUND);
    explicit
        .get("/api/ignored-duplicate")
        .await
        .unwrap()
        .assert_status(http::StatusCode::NOT_FOUND);
    let mut disabled = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok(std::collections::BTreeMap::from([(
                "horizon".into(),
                json!({"routes":false}),
            )]))
        })
        .with_providers(vec![Box::new(HorizonServiceProvider)])
    })
    .unwrap();
    disabled
        .get("/horizon")
        .await
        .unwrap()
        .assert_status(http::StatusCode::NOT_FOUND);
}

struct Stamp;
impl openrust::contracts::http::Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
        next: openrust::http::Next<'a>,
    ) -> openrust::http::HttpFuture<'a, Result<openrust::http::Response, openrust::http::HttpError>>
    {
        Box::pin(async move {
            let response = next.run(request).await;
            Ok(response.header(
                http::HeaderName::from_static("x-horizon-middleware"),
                http::HeaderValue::from_static("configured"),
            ))
        })
    }
}
#[tokio::test]
async fn configured_named_middleware_runs_on_automatically_registered_routes() {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok(std::collections::BTreeMap::from([(
                "horizon".into(),
                json!({"middleware":["stamp"]}),
            )]))
        })
        .with_middleware(|m| {
            m.alias("stamp", std::sync::Arc::new(Stamp));
        })
        .with_providers(vec![Box::new(HorizonServiceProvider)])
    })
    .unwrap();
    app.get("/horizon")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN)
        .assert_header("x-horizon-middleware", "configured");
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn environment_prefixes_isolate_two_apps_on_one_global_redis_prefix() {
    use openrust::config::{ConfigError, Value};
    fn config(
        _: &std::path::Path,
        _: &Environment,
    ) -> Result<std::collections::BTreeMap<String, Value>, ConfigError> {
        assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
        Ok(std::collections::BTreeMap::from([(
            "database".into(),
            json!({"redis":{"options":{"prefix":"shared-acceptance:"},"default":{"url":std::env::var("HORIZON_TEST_REDIS_URL").unwrap()}}}),
        )]))
    }
    let mut apps = Vec::new();
    for prefix in ["application_one:", "application_two:"] {
        let mut app = TestApp::create(|b| {
            b.with_config(config)
                .with_providers(vec![Box::new(HorizonServiceProvider)])
        })
        .unwrap();
        std::fs::write(
            app.root().join(".env"),
            format!("HORIZON_PREFIX={prefix}\n"),
        )
        .unwrap();
        app.boot().await.unwrap();
        apps.push(app);
    }
    let one = apps[0]
        .app()
        .make::<openrust_horizon::Repository>()
        .await
        .unwrap();
    let two = apps[1]
        .app()
        .make::<openrust_horizon::Repository>()
        .await
        .unwrap();
    one.pause().await.unwrap();
    assert!(one.paused().await.unwrap());
    assert!(!two.paused().await.unwrap());
    one.r#continue().await.unwrap();
    two.pause().await.unwrap();
    assert!(!one.paused().await.unwrap());
    assert!(two.paused().await.unwrap());
    two.r#continue().await.unwrap();
}
