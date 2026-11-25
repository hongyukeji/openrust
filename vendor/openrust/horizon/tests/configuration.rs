#[path = "../config/horizon.rs"]
mod configuration;
use openrust::{config::Environment, foundation::configuration::Routing, testing::TestApp};
use openrust_horizon::{Horizon, HorizonServiceProvider, MasterSupervisor};
use serde_json::json;

#[test]
fn provisioning_plan_keeps_environment_order_and_merges_default_options() {
    use openrust::config::Repository;
    use openrust_horizon::{ProvisioningPlan, SupervisorOptions};
    use std::collections::BTreeMap;

    let config = Repository::new(BTreeMap::from([
        (
            "horizon".into(),
            json!({
                "defaults": {"main": {"queue": ["initial", "retained"], "max_processes": 2}},
                "environments": {
                    "testing*": {"main": {"queue": ["override"], "processes": 1}, "disabled": {"maxProcesses": 0}},
                    "testing": {"excluded": {"maxProcesses": 1}},
                },
            }),
        ),
        (
            "queue".into(),
            json!({"connections": {"redis": {"driver": "redis", "retry_after": 90, "queue": "fallback"}}}),
        ),
    ]));
    let plan = ProvisioningPlan::new("North:東京 1-uP19", &config).unwrap();
    assert_eq!(
        plan.environments().collect::<Vec<_>>(),
        ["testing*", "testing"]
    );
    assert!(plan.has_environment("testing"));
    assert!(!plan.has_environment("testing-west"));
    assert!(plan.options_for("testing-west", "main").is_none());
    assert!(plan.options_for("testing", "absent").is_none());
    let main = plan.options_for("testing*", "main").unwrap();
    assert_eq!(main.name, "North:東京 1-uP19:main");
    assert_eq!(main.queue, ["override", "retained"]);
    assert_eq!(main.max_processes, 1);
    assert_eq!(main.parent_id, std::process::id());
    assert_eq!(
        plan.options_for("testing*", "disabled")
            .unwrap()
            .max_processes,
        0
    );
    assert_eq!(
        plan.options_for("testing", "excluded").unwrap().queue,
        ["fallback"]
    );
    assert_eq!(plan.to_supervisor_options().next().unwrap().1.len(), 2);
    let active = SupervisorOptions::from_config_for_environment(&config, "testing").unwrap();
    assert_eq!(
        active.keys().map(String::as_str).collect::<Vec<_>>(),
        ["main"]
    );
    assert_eq!(active["main"].queue, main.queue);
}

#[tokio::test]
async fn master_names_use_a_process_token_and_an_application_local_resolver() {
    use openrust::{
        http::{HttpError, HttpFuture, Request, Response},
        support::{Str, facades::Route},
    };

    fn custom(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            MasterSupervisor::determine_name_using(|| "first.cluster".into()).unwrap();
            assert_eq!(MasterSupervisor::basename().unwrap(), "first.cluster");
            let first = MasterSupervisor::process_name().unwrap();
            assert_eq!(MasterSupervisor::process_name().unwrap(), first);
            let plan = openrust_horizon::ProvisioningPlan::get(&first).unwrap();
            assert!(plan.environments().any(|name| name == "production"));
            let queue = format!("master:{first}");
            assert_eq!(MasterSupervisor::command_queue().unwrap(), queue);
            for default in [None, Some(""), Some("0")] {
                assert_eq!(MasterSupervisor::command_queue_for(default).unwrap(), queue);
            }
            assert_eq!(
                MasterSupervisor::command_queue_for(Some("North:東京")).unwrap(),
                "master:North:東京"
            );
            MasterSupervisor::determine_name_using(|| "second_cluster".into()).unwrap();
            let second = MasterSupervisor::process_name().unwrap();
            let (_, token) = second.rsplit_once('-').unwrap();
            for basename in [String::new(), "North:東京 1".into(), "a".repeat(124)] {
                let captured = basename.clone();
                MasterSupervisor::determine_name_using(move || captured.clone()).unwrap();
                assert_eq!(MasterSupervisor::basename().unwrap(), basename);
                assert_eq!(
                    MasterSupervisor::process_name().unwrap(),
                    format!("{basename}-{token}")
                );
            }
            Response::json(http::StatusCode::OK, json!([first, second]))
        })
    }
    fn standard(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let host = hostname::get().unwrap().into_string().unwrap();
            assert_eq!(MasterSupervisor::basename().unwrap(), Str::slug(&host));
            Response::json(
                http::StatusCode::OK,
                json!(MasterSupervisor::process_name().unwrap()),
            )
        })
    }
    let mut custom_app = TestApp::create(|builder| {
        builder
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| Route::get("/master-name", custom)))
    })
    .unwrap();
    let mut default_app = TestApp::create(|builder| {
        builder
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| Route::get("/master-name", standard)))
    })
    .unwrap();

    let custom = custom_app.get("/api/master-name").await.unwrap();
    let [first, second]: [String; 2] = serde_json::from_slice(custom.body()).unwrap();
    let token = first.strip_prefix("first.cluster-").unwrap();
    assert_eq!(token.len(), 4);
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert_eq!(second, format!("second_cluster-{token}"));
    let standard = default_app.get("/api/master-name").await.unwrap();
    let standard: String = serde_json::from_slice(standard.body()).unwrap();
    let host = hostname::get().unwrap().into_string().unwrap();
    assert_eq!(standard, format!("{}-{token}", Str::slug(&host)));
}

#[tokio::test]
async fn public_supervisor_events_deliver_explicit_payloads() {
    use openrust::{
        contracts::events::{Dispatcher as _, Event, Listener},
        events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
        process::Factory,
    };
    use openrust_horizon::{
        MasterSupervisorReviving, SupervisorOptions, SupervisorProcess, SupervisorProcessRestarting,
    };
    use std::sync::{Arc, Mutex};

    struct Observer {
        process: Arc<SupervisorProcess>,
        records: Mutex<Vec<serde_json::Value>>,
    }

    impl Listener for Observer {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let record = if let Some(event) = event.downcast_ref::<MasterSupervisorReviving>() {
                    json!({"master": event.master})
                } else {
                    let event = event.downcast_ref::<SupervisorProcessRestarting>().unwrap();
                    assert!(Arc::ptr_eq(&event.process, &self.process));
                    assert!(event.process.process().unwrap().id().is_some());
                    json!({
                        "supervisor": event.process.name(),
                        "queue": event.process.options().queue,
                        "dead": event.process.is_dead()
                    })
                };
                self.records.lock().unwrap().push(record);
                Ok(None)
            })
        }
    }

    let mut app =
        TestApp::create(|builder| builder.with_providers(vec![Box::new(HorizonServiceProvider)]))
            .unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    let command = [
        std::env::current_exe().unwrap().into_os_string(),
        "--list".into(),
    ];
    let child = Factory::new().command(command).quietly().start().unwrap();
    let process = Arc::new(SupervisorProcess::new(
        SupervisorOptions {
            name: "master:supervisor".into(),
            queue: vec!["primary".into()],
            ..Default::default()
        },
        Some(child),
    ));
    let observer = Arc::new(Observer {
        process: process.clone(),
        records: Mutex::default(),
    });
    for name in [
        MasterSupervisorReviving::NAME,
        SupervisorProcessRestarting::NAME,
    ] {
        assert!(!dispatcher.has_listeners(name));
        dispatcher
            .listen(name, observer.clone(), Delivery::Sync)
            .unwrap();
    }
    let mut context = EventContext::new();
    dispatcher
        .dispatch(
            Arc::new(MasterSupervisorReviving::new("master")),
            &mut context,
        )
        .await
        .unwrap();
    dispatcher
        .dispatch(
            Arc::new(SupervisorProcessRestarting::new(process)),
            &mut context,
        )
        .await
        .unwrap();
    assert_eq!(
        observer.records.lock().unwrap().as_slice(),
        [
            json!({"master": "master"}),
            json!({"supervisor": "master:supervisor", "queue": ["primary"], "dead": false}),
        ],
    );
}

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
