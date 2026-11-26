use openrust::view::ViewFinder as _;
use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Route,
    testing::TestApp,
};
use openrust_pulse::{Pulse, PulseServiceProvider, storage::DatabaseStorage};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
fn config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    std::fs::write(root.join("pulse.sqlite"), []).unwrap();
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"local","url":"http://localhost","debug":true,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"}),
        ),
        (
            "cache".into(),
            json!({"default":"file","stores":{"file":{"driver":"file","path":root.join("cache/data"),"lock_path":root.join("cache/locks")}}}),
        ),
        ("session".into(), json!({"driver":"array"})),
        (
            "auth".into(),
            json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"pulse-test"}}}),
        ),
        (
            "database".into(),
            json!({"default":"test","connections":{"test":{"driver":"sqlite","database":root.join("pulse.sqlite"),"max_connections":1}}}),
        ),
        (
            "pulse".into(),
            json!({"enabled":true,"ingest":{"driver":"storage","buffer":5000,"trim":{"lottery":[0,1]}},"storage":{"driver":"database","database":{"chunk":2}},"recorders":{"SlowRequests":{"enabled":true,"threshold":0,"sample_rate":1},"SlowQueries":false,"CacheInteractions":false,"UserRequests":false,"Servers":{}}}),
        ),
    ]))
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/items/{id}", item)?;
    Route::get("/observe", observed)?;
    Route::get("/metric-example", metric_example)?;
    Route::get("/query-source", query_source)?;
    Ok(())
}

fn query_source(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::support::facades::Db;

        let line = line!() + 1;
        Db::select("select 42 as pulse_source_probe", &[]).await?;
        Response::json(
            http::StatusCode::OK,
            json!(format!("{}:{line}", file!().replace('\\', "/"))),
        )
    })
}
fn item(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::from("ok")) })
}

fn livewire_routes() -> Result<(), openrust::routing::RoutingError> {
    Route::group(openrust::routing::RouteGroup::new(), |router| {
        router
            .post("/update", decoded_request)?
            .name("app.livewire.update");
        router.post("/unread", item)?.name("unread.livewire.update");
        router
            .post("/ordinary", decoded_request)?
            .name("ordinary.update");
        Ok(())
    })
}

fn decoded_request(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        openrust::support::facades::Auth::user(request).await?;
        request.json(None).await?;
        Ok(Response::from("ok"))
    })
}

struct SlowTermination;

impl openrust::contracts::http::Middleware for SlowTermination {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: openrust::http::Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move { Ok(next.run(request).await) })
    }
}

impl openrust::contracts::http::TerminableMiddleware for SlowTermination {
    fn terminate<'a>(
        &'a self,
        _: &'a openrust::http::RequestParts,
        _: &'a openrust::http::ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(25)))
    }
}

fn failed_after_defer(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        openrust::support::facades::Auth::user(request).await?;
        let _ = openrust::support::defer(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(45)).await;
                let _ = openrust::support::defer(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(45)).await;
                        Ok::<_, std::io::Error>(())
                    },
                    None,
                    true,
                )
                .unwrap();
                Ok::<_, std::io::Error>(())
            },
            None,
            true,
        )
        .unwrap();
        Response::json(openrust::http::StatusCode::UNPROCESSABLE_ENTITY, json!({}))
    })
}

#[tokio::test]
async fn request_recorders_include_termination_and_nested_deferred_work() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"] = json!({
                    "SlowRequests":{"threshold":100},
                    "UserRequests":{}
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_middleware(|middleware| {
                middleware.append_terminable(Arc::new(SlowTermination));
            })
            .with_routing(Routing::default().api(|| {
                Route::get("/terminal", failed_after_defer)?;
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/terminal?user=1")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::UNPROCESSABLE_ENTITY);
    let store = app.app().make::<DatabaseStorage>().await.unwrap();
    let connection = store.connection();
    let rows = connection
        .table("pulse_entries")
        .r#where("type", "=", "slow_request")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let Some(openrust::database::Value::Integer(duration)) = rows[0].get("value") else {
        panic!("missing slow request duration")
    };
    assert!(*duration >= 115);
    for kind in ["user_request", "slow_user_request"] {
        assert_eq!(
            connection
                .table("pulse_entries")
                .r#where("type", "=", kind)
                .r#where("key", "=", "42")
                .count("*")
                .await
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn request_recorders_attribute_named_livewire_updates_to_the_decoded_page() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"] = json!({
                    "SlowRequests":{"threshold":0,"ignore":["#^/ignored$#"]},
                    "UserRequests":{"ignore":["#^/ignored$#"]}
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_routing(Routing::default().api(livewire_routes))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    for (route, snapshot, user) in [
        ("update", r#"{"memo":{"path":"//orders"}}"#, 1),
        ("update", r#"{"memo":{"path":"ignored"}}"#, 1),
        ("update", "malformed", 1),
        ("update", r#"{"memo":{}}"#, 1),
        ("unread", r#"{"memo":{"path":"never-read"}}"#, 1),
        ("ordinary", r#"{"memo":{"path":"also-unused"}}"#, 1),
        ("update", r#"{"memo":{"path":"zero"}}"#, 0),
    ] {
        app.call(
            http::Request::builder()
                .method("POST")
                .uri(format!("/api/{route}?user={user}"))
                .header(
                    "content-type",
                    if route == "unread" {
                        "application/octet-stream"
                    } else {
                        "application/json"
                    },
                )
                .body(openrust::http::Body::from(
                    json!({"components":[{"snapshot":snapshot}]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .assert_ok();
    }
    let store = app.app().make::<DatabaseStorage>().await.unwrap();
    let connection = store.connection();
    let rows = connection
        .table("pulse_entries")
        .r#where("type", "=", "slow_request")
        .get()
        .await
        .unwrap();
    let actual = rows
        .iter()
        .map(|row| {
            let Some(openrust::database::Value::Text(key)) = row.get("key") else {
                panic!("missing slow request key")
            };
            serde_json::from_str::<serde_json::Value>(key).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), 5);
    for expected in [
        json!(["POST", "/orders", "via /api/update"]),
        json!(["POST", "/api/update", "provider::decoded_request"]),
        json!(["POST", "/api/unread", "provider::item"]),
        json!(["POST", "/api/ordinary", "provider::decoded_request"]),
        json!(["POST", "/zero", "via /api/update"]),
    ] {
        assert!(
            actual.contains(&expected),
            "missing {expected} in {actual:?}"
        );
    }
    for kind in ["user_request", "slow_user_request"] {
        assert_eq!(
            connection
                .table("pulse_entries")
                .r#where("type", "=", kind)
                .r#where("key", "=", "42")
                .count("*")
                .await
                .unwrap(),
            3
        );
    }
    for (kind, count) in [("user_request", 1), ("slow_user_request", 0)] {
        assert_eq!(
            connection
                .table("pulse_entries")
                .r#where("type", "=", kind)
                .r#where("key", "=", "0")
                .count("*")
                .await
                .unwrap(),
            count
        );
    }
}
#[tokio::test]
async fn request_recorders_apply_dashboard_path_filters_to_native_metrics() {
    use openrust::{
        contracts::foundation::Application as _, database::Value as DbValue, support::facades::Auth,
    };

    fn report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::user(request).await?;
            Ok(Response::from("ok"))
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("app").unwrap()["env"] = json!("production");
                let pulse = values.get_mut("pulse").unwrap();
                pulse["path"] = json!("operations/pulse");
                pulse["recorders"] = json!({
                    "SlowRequests":{"threshold":0,"ignore":["#^/operations/pulse$#"]},
                    "UserRequests":{"ignore":["#^/operations/pulse$#"]}
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_routing(Routing::default().web(|| {
                Route::get("/operations/pulse-report", report)?;
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.app()
        .make::<openrust::auth::access::GateRegistry>()
        .await
        .unwrap()
        .define_guest("viewPulse", |_, _| {
            Box::pin(async { Ok(openrust::auth::access::Response::allow()) })
        });
    for url in [
        "/operations/pulse/api/metrics?user=1",
        "/operations/pulse/api/metrics?user=1",
        "/operations/pulse-report?user=1",
    ] {
        app.get(url).await.unwrap().assert_ok();
    }
    let store = app.app().make::<DatabaseStorage>().await.unwrap();
    let connection = store.connection();
    for kind in ["slow_request", "user_request", "slow_user_request"] {
        let rows = connection
            .table("pulse_entries")
            .r#where("type", "=", kind)
            .get()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "unexpected {kind} entries: {rows:?}");
        let key = if kind == "slow_request" {
            json!(["GET", "/operations/pulse-report", "Closure"]).to_string()
        } else {
            "42".into()
        };
        assert_eq!(rows[0].get("key"), Some(&DbValue::Text(key)));
    }

    let config = app.app().config().unwrap();
    for name in ["SlowRequests", "UserRequests"] {
        config
            .set(&format!("pulse.recorders.{name}.ignore"), json!([]))
            .unwrap();
    }
    app.get("/operations/pulse/api/metrics?user=1")
        .await
        .unwrap()
        .assert_ok();
    let rows = connection
        .table("pulse_entries")
        .r#where("type", "=", "slow_request")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| {
        row.get("key")
            == Some(&DbValue::Text(
                json!([
                    "GET",
                    "/operations/pulse",
                    "via /operations/pulse/api/metrics"
                ])
                .to_string(),
            ))
    }));
    for kind in ["user_request", "slow_user_request"] {
        assert_eq!(
            connection
                .table("pulse_entries")
                .r#where("type", "=", kind)
                .r#where("key", "=", "42")
                .count("*")
                .await
                .unwrap(),
            2
        );
    }
}

struct AuthProvider;
impl openrust::support::ServiceProvider for AuthProvider {
    fn boot<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            app.make::<openrust::auth::AuthManager>()
                .await?
                .via_request("pulse-test", |request| {
                    let user = request
                        .uri()
                        .query()
                        .and_then(|query| {
                            if query.contains("user=1") {
                                Some(42)
                            } else if query.contains("user=0") {
                                Some(0)
                            } else {
                                None
                            }
                        })
                        .map(|id| {
                            Box::new(Identity(id))
                                as Box<dyn openrust::contracts::auth::Authenticatable>
                        });
                    Box::pin(async move { Ok(user) })
                });
            Ok(())
        })
    }
}
fn app() -> TestApp {
    TestApp::create(|b| {
        b.with_config(config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_routing(Routing::default().api(routes))
    })
    .unwrap()
}

fn recorder_database_config(
    root: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let mut values = config(root, env)?;
    let url = match std::env::var("PULSE_TEST_DRIVER").as_deref() {
        Ok("pgsql") => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            Some(std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap())
        }
        Ok("mysql") => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            Some(std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap())
        }
        Ok("sqlite") | Err(std::env::VarError::NotPresent) => None,
        driver => panic!("unexpected Pulse test driver {driver:?}"),
    };
    if let Some(url) = url {
        values.get_mut("database").unwrap()["connections"]["test"] = json!({"url":url});
    }
    Ok(values)
}

#[tokio::test]
async fn dashboard_and_partial_metrics_recheck_application_middleware() {
    use openrust::{
        contracts::http::Middleware,
        http::{Body, Next, StatusCode},
    };

    struct PulseAccess(&'static str);

    impl Middleware for PulseAccess {
        fn handle<'a>(
            &'a self,
            request: &'a mut Request,
            next: Next<'a>,
        ) -> HttpFuture<'a, Result<Response, HttpError>> {
            Box::pin(async move {
                if request
                    .headers()
                    .get("x-fixture-access")
                    .and_then(|value| value.to_str().ok())
                    != Some(self.0)
                {
                    return Err(HttpError::AuthorizationDenied("没有看板访问权限".into()));
                }
                Ok(next.run(request).await)
            })
        }
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("pulse").unwrap()["middleware"] = json!([
                    "web",
                    "openrust_pulse::http::middleware::Authorize",
                    "fixture.pulse-access:pulse-view"
                ]);
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_middleware(|middleware| {
                middleware.alias(
                    "fixture.pulse-access:pulse-view",
                    Arc::new(PulseAccess("pulse-view")),
                );
            })
            .with_routing(Routing::default().api(routes))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();

    let card = "/pulse/api/metrics?cards=cache";
    for (path, access, expected) in [
        ("/pulse", "denied", StatusCode::FORBIDDEN),
        ("/pulse", "pulse-view", StatusCode::OK),
        (card, "pulse-view", StatusCode::OK),
        (card, "denied", StatusCode::FORBIDDEN),
        (card, "pulse-view", StatusCode::OK),
    ] {
        let request = http::Request::builder()
            .method("GET")
            .uri(path)
            .header("x-fixture-access", access)
            .body(Body::empty())
            .unwrap();
        app.call(request).await.unwrap().assert_status(expected);
    }
}

#[tokio::test]
async fn dashboard_rechecks_a_browser_session_after_sign_out() {
    use openrust::{
        auth::access::{GateRegistry, Response as GateResponse},
        contracts::auth::StatefulGuard,
        database::{DatabaseManager, Value as DbValue},
        http::{Body, StatusCode},
        routing::RoutingError,
        support::facades::Auth,
    };

    fn sign_in(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::session_guard(request, None)?
                .login_using_id(&42_u64.into(), false)
                .await?;
            Ok(Response::empty(StatusCode::NO_CONTENT))
        })
    }

    fn sign_out(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::session_guard(request, None)?.logout().await?;
            Ok(Response::empty(StatusCode::NO_CONTENT))
        })
    }

    fn session_routes() -> Result<(), RoutingError> {
        Route::get("/fixture/sign-in", sign_in)?;
        Route::get("/fixture/sign-out", sign_out)?;
        Ok(())
    }

    fn request(path: &str, cookie: &str) -> http::Request<Body> {
        http::Request::builder()
            .method("GET")
            .uri(path)
            .header(http::header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = usage_user_config(root, env)?;
                values.get_mut("app").unwrap()["env"] = json!("production");
                values.get_mut("auth").unwrap()["guards"]["web"]["driver"] = json!("session");
                values.get_mut("session").unwrap()["cookie"] = json!("pulse-session-fixture");
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().web(session_routes))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    application
        .make::<GateRegistry>()
        .await
        .unwrap()
        .define_guest("viewPulse", |user, _| {
            Box::pin(async move {
                Ok(if user.is_some() {
                    GateResponse::allow()
                } else {
                    GateResponse::deny("没有看板访问权限")
                })
            })
        });
    let connection = application
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .get_schema_builder()
        .create("pulse_test_users", |table| {
            table.id();
            table.string("name", 255);
            table.string("password", 255);
            table.string("remember_token", 100).nullable();
        })
        .await
        .unwrap();
    connection
        .table("pulse_test_users")
        .insert(&[BTreeMap::from([
            ("id".into(), DbValue::from(42_i64)),
            ("name".into(), DbValue::from("Pulse session user")),
            ("password".into(), DbValue::from("用于登录测试的固定指纹")),
        ])])
        .await
        .unwrap();

    let card = "/pulse/api/metrics?cards=cache";
    app.get("/pulse")
        .await
        .unwrap()
        .assert_status(StatusCode::FORBIDDEN);
    let signed_in = app.get("/fixture/sign-in").await.unwrap();
    signed_in.assert_status(StatusCode::NO_CONTENT);
    let cookie = signed_in
        .headers()
        .get_all(http::header::SET_COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .filter_map(|header| header.split(';').next())
        .find(|cookie| cookie.starts_with("pulse-session-fixture="))
        .unwrap();
    for path in ["/pulse", card] {
        app.call(request(path, cookie)).await.unwrap().assert_ok();
    }
    app.call(request("/fixture/sign-out", cookie))
        .await
        .unwrap()
        .assert_status(StatusCode::NO_CONTENT);
    for path in [card, "/pulse"] {
        app.call(request(path, cookie))
            .await
            .unwrap()
            .assert_status(StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn queued_artisan_recorders_filter_by_display_name() {
    use openrust::{
        contracts::queue::JobHandler,
        database::Value as DbValue,
        queue::{JobContext, Payload, QueueFuture, QueueResult},
        support::facades::{Artisan, Auth, Bus},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ChainStep(AtomicUsize);
    impl JobHandler for ChainStep {
        fn handle<'a>(
            &'a self,
            _: &'a mut JobContext<'_>,
            _: &'a Payload,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        }
    }

    fn transport() -> (&'static str, serde_json::Value) {
        if let Ok(url) = std::env::var("PULSE_TEST_SQS_URL") {
            return (
                "sqs",
                json!({"driver":"sqs","queue":url,"endpoint":std::env::var("PULSE_TEST_SQS_ENDPOINT").unwrap(),"region":"us-east-1","key":"test","secret":"test","block_for":0}),
            );
        }
        if let Ok(port) = std::env::var("PULSE_TEST_BEANSTALKD_PORT") {
            return (
                "beanstalkd",
                json!({"driver":"beanstalkd","host":"127.0.0.1","port":port.parse::<u16>().unwrap(),"block_for":0}),
            );
        }
        (
            "database",
            json!({"driver":"database","table":"jobs","retry_after":90}),
        )
    }

    fn enqueue(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::user(request).await?;
            for name in ["pulse-probe:ignored", "pulse-probe:kept"] {
                Artisan::queue(name, &[]).dispatch().await.unwrap();
            }
            Ok(Response::from("queued"))
        })
    }

    fn enqueue_chain(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            Bus::chain([0, 1].map(|index| Payload::new("pulse-chain-step", index).unwrap()))
                .on_connection(transport().0)
                .dispatch()
                .await
                .unwrap();
            Ok(Response::from("queued"))
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                let (driver, settings) = transport();
                values.insert(
                    "queue".into(),
                    json!({
                        "default":driver,
                        "connections":{(driver):settings},
                        "failed":{"driver":"null"}
                    }),
                );
                let ignore = json!(["/^pulse-probe:ignored$/"]);
                values.get_mut("pulse").unwrap()["recorders"] = json!({
                    "Queues":{"ignore":ignore},
                    "UserJobs":{"ignore":ignore},
                    "SlowJobs":{"ignore":ignore,"threshold":0}
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_routing(
                Routing::default()
                    .api(|| {
                        Route::get("/queue-commands", enqueue)?;
                        Route::get("/queue-chain", enqueue_chain)?;
                        Ok(())
                    })
                    .commands(|| {
                        Artisan::command("pulse-probe:ignored", |_| {})?;
                        Artisan::command("pulse-probe:kept", |_| {})?;
                        Ok(())
                    }),
            )
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let connection = storage.connection();
    let (driver, _) = transport();
    if driver == "database" {
        connection
            .get_schema_builder()
            .create("jobs", |table| {
                table.id();
                table.string("queue", 255).index();
                table.long_text("payload");
                table.unsigned_integer("attempts");
                table.unsigned_big_integer("reserved_at").nullable();
                table.unsigned_big_integer("available_at");
                table.unsigned_big_integer("created_at");
            })
            .await
            .unwrap();
    }

    app.get("/api/queue-commands?user=1")
        .await
        .unwrap()
        .assert_ok();
    let manager = app
        .app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap();
    let queue = manager.connection(Some(driver)).unwrap();
    assert_eq!(queue.size(None).await.unwrap(), 2);
    if driver == "database" {
        let queued = connection
            .table("jobs")
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        assert_eq!(queued.len(), 2);
        for (row, name) in queued
            .iter()
            .zip(["pulse-probe:ignored", "pulse-probe:kept"])
        {
            let Some(DbValue::Text(raw)) = row.get("payload") else {
                panic!("missing payload")
            };
            let payload: Payload = serde_json::from_str(raw).unwrap();
            assert_eq!(payload.name(), "artisan.queued_command");
            assert_eq!(payload.display_name(), name);
        }
    }

    app.artisan([
        "queue:work",
        driver,
        "--stop-when-empty",
        "--sleep=0",
        "--memory=0",
    ])
    .await
    .assert_successful();
    assert_eq!(queue.size(None).await.unwrap(), 0);
    let counts = storage
        .aggregate_total(
            &["queued", "processing", "processed", "failed"].map(str::to_owned),
            "count",
            3600,
        )
        .await
        .unwrap();
    for kind in ["queued", "processing", "processed"] {
        assert_eq!(counts.get(kind), Some(&Some(1.0)), "{kind}");
    }
    assert!(!counts.contains_key("failed"));
    for (kind, key) in [("user_job", "42"), ("slow_job", "pulse-probe:kept")] {
        let rows = connection
            .table("pulse_entries")
            .r#where("type", "=", kind)
            .get()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "{kind}");
        assert_eq!(rows[0].get("key"), Some(&DbValue::Text(key.into())));
    }

    if driver != "database" {
        let handler = Arc::new(ChainStep(AtomicUsize::new(0)));
        manager
            .register("pulse-chain-step", handler.clone())
            .unwrap();
        app.get("/api/queue-chain").await.unwrap().assert_ok();
        let before = storage
            .aggregate_total(&["queued".into()], "count", 3600)
            .await
            .unwrap();
        assert_eq!(before["queued"], Some(2.0));
        app.artisan([
            "queue:work",
            driver,
            "--stop-when-empty",
            "--sleep=0",
            "--memory=0",
        ])
        .await
        .assert_successful();
        assert_eq!(handler.0.load(Ordering::Relaxed), 2);
        assert_eq!(queue.size(None).await.unwrap(), 0);
        let counts = storage
            .aggregate_total(&["queued".into(), "processed".into()], "count", 3600)
            .await
            .unwrap();
        assert_eq!(counts["queued"], Some(3.0));
        assert_eq!(counts["processed"], Some(3.0));
    }
}

#[tokio::test]
async fn slow_queries_use_the_query_event_source_and_respect_the_location_switch() {
    use openrust::contracts::foundation::Application as _;

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"]["SlowQueries"] =
                    json!({"threshold":0,"location":true});
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(routes))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let response = app.get("/api/query-source").await.unwrap();
    response.assert_ok();
    let mut expected = response.json::<String>().unwrap();
    if let Some(prefix) = option_env!("OPENRUST_PULSE_TEST_INTERNAL_SOURCE") {
        assert!(expected.starts_with(prefix), "{expected}");
        expected.clear();
    }
    app.app()
        .config()
        .unwrap()
        .set("pulse.recorders.SlowQueries.location", false.into())
        .unwrap();
    app.get("/api/query-source").await.unwrap().assert_ok();

    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let rows = storage
        .connection()
        .table("pulse_entries")
        .r#where("type", "=", "slow_query")
        .get()
        .await
        .unwrap();
    let locations: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            let openrust::database::Value::Text(key) = row.get("key")? else {
                return None;
            };
            let key: serde_json::Value = serde_json::from_str(key).unwrap();
            (key[0] == "select 42 as pulse_source_probe").then(|| key[1].clone())
        })
        .collect();
    assert_eq!(locations.len(), 2);
    assert!(locations.contains(&json!(expected)));
    assert!(locations.contains(&serde_json::Value::Null));
}

#[tokio::test]
async fn provider_is_lazy_and_native_http_lifecycles_record_canonical_route_paths() {
    let mut app = app();
    app.boot().await.unwrap();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    assert!(
        !storage
            .connection()
            .get_schema_builder()
            .has_table("pulse_entries")
            .await
            .unwrap()
    );
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/items/12").await.unwrap().assert_ok();
    app.get("/api/items/99").await.unwrap().assert_ok();
    app.get("/api/missing")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::NOT_FOUND);
    let rows = storage
        .connection()
        .table("pulse_entries")
        .r#where("type", "=", "slow_request")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let Some(openrust::database::Value::Text(key)) = row.get("key") else {
            panic!()
        };
        let key: serde_json::Value = serde_json::from_str(key).unwrap();
        assert_eq!(key, json!(["GET", "/api/items/{id}", "provider::item"]));
    }
    app.artisan(["vendor:publish", "--tag=pulse-config"])
        .await
        .assert_successful();
    let config = app.root().join("config/pulse.rs");
    std::fs::write(&config, "// custom").unwrap();
    app.artisan(["vendor:publish", "--tag=pulse-config"])
        .await
        .assert_successful();
    assert_eq!(std::fs::read_to_string(config).unwrap(), "// custom");
}
#[tokio::test]
async fn scoped_buffers_share_filters_but_never_measure_pulse_own_storage() {
    let mut app = app();
    app.boot().await.unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    let pulse = application.make::<Pulse>().await.unwrap();
    pulse.filter(|item| Ok(!matches!(item,openrust_pulse::Item::Entry(e) if e.key=="hidden")));
    pulse.record("custom", "hidden", None, None).await.count();
    pulse.record("custom", "shown", None, None).await.count();
    assert_eq!(pulse.ingest().await, 1);
    let storage = application.make::<DatabaseStorage>().await.unwrap();
    assert_eq!(
        storage
            .connection()
            .table("pulse_entries")
            .count("*")
            .await
            .unwrap(),
        1
    );
    let storage_dyn: Arc<dyn openrust_pulse::storage::Storage> = storage.clone();
    let local = Pulse::new(
        storage_dyn.clone(),
        Arc::new(openrust_pulse::ingests::StorageIngest(storage_dyn)),
    )
    .with_buffer(1)
    .with_trim_lottery(openrust::support::Lottery::odds(0, 1).unwrap());
    local.record("buffer", "one", Some(1), None).await.count();
    // Upstream flushes before the second record's returned builder is mutated.
    local.record("buffer", "two", Some(2), None).await.count();
    assert!(!local.wants_ingesting());
    local
        .ignore(async {
            local.record("ignored", "none", None, None).await.count();
        })
        .await;
    assert!(!local.wants_ingesting());
    assert!(local.is_recording());
    local
        .lazy(|pulse| {
            Box::pin(async move {
                pulse.record("lazy", "one", None, None).await.count();
                Ok(())
            })
        })
        .await;
    assert_eq!(local.ingest().await, 1);
}

fn dashboard_cache_config(
    root: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let mut values = recorder_database_config(root, env)?;
    values.get_mut("pulse").unwrap()["recorders"]["SlowRequests"] = false.into();
    if let Ok(url) = std::env::var("PULSE_TEST_CARD_REDIS_URL") {
        let prefix = root.file_name().unwrap().to_string_lossy();
        values.get_mut("database").unwrap()["redis"] = json!({
            "options":{"prefix":format!("{prefix}:")},
            "default":{"url":url},
        });
        values.get_mut("cache").unwrap()["stores"]["cards"] =
            json!({"driver":"redis","connection":"default"});
        values.get_mut("pulse").unwrap()["cache"] = json!("cards");
    }
    Ok(values)
}

#[tokio::test]
async fn dashboard_selected_cards_skip_unrequested_queries_and_keep_the_complete_response() {
    use openrust::{contracts::foundation::Application as _, http::StatusCode};

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(dashboard_cache_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let target = "/pulse/api/metrics";
    let all = app.get(target).await.unwrap();
    all.assert_ok();
    let all = all.json::<serde_json::Value>().unwrap();
    for card in ["servers", "requests", "jobs", "cache", "queues"] {
        assert!(all.get(card).is_some(), "{card}");
        assert!(all["query_timing"].get(card).is_some(), "{card}");
    }
    for invalid in ["", "servers,unknown", "queues,"] {
        app.get(&format!("{target}?cards={invalid}"))
            .await
            .unwrap()
            .assert_status(StatusCode::BAD_REQUEST);
    }

    let config = app.app().config().unwrap();
    config
        .set(
            "pulse.dashboard.servers.ignore_after",
            json!("invalid-duration"),
        )
        .unwrap();
    for url in [target.to_owned(), format!("{target}?cards=servers")] {
        app.get(&url)
            .await
            .unwrap()
            .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let subset = app
        .get(&format!("{target}?cards=cache,queues&usage=invalid"))
        .await
        .unwrap();
    subset.assert_ok();
    let subset = subset.json::<serde_json::Value>().unwrap();
    for key in ["cache", "cache_totals", "queues", "queue_sample_rate"] {
        assert_eq!(subset.get(key), all.get(key), "{key}");
    }
    assert_eq!(subset["sample_rates"], json!({}));
    for key in [
        "servers",
        "server_graphs",
        "requests",
        "jobs",
        "slow_requests",
    ] {
        assert!(subset.get(key).is_none(), "{key}");
        assert!(subset["query_timing"].get(key).is_none(), "{key}");
    }
    for key in ["cache", "cache_totals", "queues"] {
        assert_eq!(subset["query_timing"][key], all["query_timing"][key]);
    }
}

#[tokio::test]
async fn dashboard_slow_cards_cache_queries_by_card_order_and_period() {
    use openrust::{
        contracts::cache::Repository as _,
        support::{Date, DateTimezone},
    };

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(dashboard_cache_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.travel_to(Date::from_utc(
        "2026-11-26T13:00:00.654321Z".parse().unwrap(),
        DateTimezone::UTC,
    ));
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let kinds = [
        ("slow_requests", "slow_request"),
        ("slow_queries", "slow_query"),
        ("slow_jobs", "slow_job"),
        ("slow_outgoing_requests", "slow_outgoing_request"),
        ("exceptions", "exception"),
    ];
    let entries = |name, value| {
        kinds.map(|(_, kind)| {
            openrust_pulse::Entry::new(timestamp, kind, name, Some(value))
                .count()
                .max()
                .into()
        })
    };
    storage.store(&entries("before", 100)).await.unwrap();
    let first = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    first.assert_ok();
    let first = first.json::<serde_json::Value>().unwrap();
    for (card, _) in kinds {
        let timing = &first["query_timing"][card];
        assert_eq!(timing["run_at"], "2026-11-26 13:00:00", "{card}");
        assert!(
            timing["time"].as_f64().is_some_and(|time| time >= 0.0),
            "{card}"
        );
    }
    if std::env::var_os("PULSE_TEST_CARD_REDIS_URL").is_some() {
        let manager = app
            .app()
            .make::<openrust::cache::CacheManager>()
            .await
            .unwrap();
        let cache = manager.store(Some("cards")).unwrap();
        assert!(
            cache
                .get("openrust:pulse:card:query:slow_requests:max:1h")
                .await
                .unwrap()
                .is_some()
        );
    }
    storage.store(&entries("after", 200)).await.unwrap();

    let order = "slow-requests=count&slow-queries=count&slow-jobs=count&slow-outgoing-requests=count&exceptions=latest";
    for (query, expected) in [
        ("period=1h".to_owned(), 1),
        (format!("period=1h&{order}"), 2),
        ("period=6h".to_owned(), 2),
    ] {
        let response = app
            .get(&format!("/pulse/api/metrics?{query}"))
            .await
            .unwrap();
        response.assert_ok();
        let response = response.json::<serde_json::Value>().unwrap();
        for (card, _) in kinds {
            let rows = response[card].as_array().unwrap();
            assert_eq!(rows.len(), expected, "{query}, {card}");
            if expected == 1 {
                assert_eq!(response[card], first[card]);
                assert_eq!(response["query_timing"][card], first["query_timing"][card]);
            }
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(5200)).await;
    app.travel_to(Date::from_utc(
        "2026-11-26T13:00:06Z".parse().unwrap(),
        DateTimezone::UTC,
    ));
    let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    response.assert_ok();
    let response = response.json::<serde_json::Value>().unwrap();
    for (card, _) in kinds {
        assert_eq!(response[card].as_array().unwrap().len(), 2, "{card}");
        assert_eq!(
            response["query_timing"][card]["run_at"], "2026-11-26 13:00:06",
            "{card}"
        );
    }
}

#[tokio::test]
async fn dashboard_usage_cache_and_queue_cards_share_five_second_query_results() {
    use openrust::contracts::foundation::Application as _;

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(dashboard_cache_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let rows = |key| {
        ["user_request", "user_job", "cache_hit", "queued"].map(|kind| {
            openrust_pulse::Entry::new(timestamp, kind, key, None)
                .count()
                .into()
        })
    };
    let config = app.app().config().unwrap();
    config
        .set("pulse.recorders.Queues.sample_rate", json!(0.4))
        .unwrap();
    let target = "/pulse/api/metrics?period=1h";
    storage.store(&rows("1")).await.unwrap();
    let first = app.get(target).await.unwrap();
    first.assert_ok();
    let first = first.json::<serde_json::Value>().unwrap();
    assert_eq!(first["queue_sample_rate"], 0.4);

    storage.store(&rows("2")).await.unwrap();
    config
        .set("pulse.recorders.Queues.sample_rate", json!(0.8))
        .unwrap();

    let replay = app.get("/pulse/api/metrics?period=1_hour").await.unwrap();
    replay.assert_ok();
    let replay = replay.json::<serde_json::Value>().unwrap();
    assert_eq!(replay["queue_sample_rate"], 0.8);
    assert_eq!(
        replay["query_timing"]["queues"],
        first["query_timing"]["queues"]
    );

    let new_period = app.get("/pulse/api/metrics?period=6h").await.unwrap();
    new_period.assert_ok();
    let new_period = new_period.json::<serde_json::Value>().unwrap();
    let cards = ["requests", "jobs", "cache", "cache_totals", "queues"];
    for card in cards {
        assert_eq!(replay[card], first[card], "{card}");
        assert_ne!(new_period[card], first[card], "{card}");
    }

    tokio::time::sleep(std::time::Duration::from_millis(5200)).await;
    let refreshed = app.get(target).await.unwrap();
    refreshed.assert_ok();
    let refreshed = refreshed.json::<serde_json::Value>().unwrap();
    for card in cards {
        assert_ne!(refreshed[card], first[card], "{card}");
    }
    assert_eq!(refreshed["cache_totals"]["hits"], 2.0);
}

#[tokio::test]
async fn dashboard_sample_rates_follow_each_recorder_without_scaling_cached_rows() {
    use openrust::{contracts::foundation::Application as _, support::php_float_cast};

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(dashboard_cache_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let store = app.app().make::<DatabaseStorage>().await.unwrap();
    let config = app.app().config().unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let configured = [
        ("requests", "user_request", "UserRequests", 0.4),
        ("slow_requests", "slow_request", "SlowRequests", 0.5),
        ("slow_queries", "slow_query", "SlowQueries", 0.25),
        ("slow_jobs", "slow_job", "SlowJobs", 0.1),
        (
            "slow_outgoing_requests",
            "slow_outgoing_request",
            "SlowOutgoingRequests",
            0.2,
        ),
        ("exceptions", "exception", "Exceptions", 0.8),
        ("jobs", "user_job", "UserJobs", 0.6),
    ];
    for (_, _, recorder, rate) in configured {
        config
            .set(
                &format!("pulse.recorders.{recorder}"),
                json!({"enabled":false,"sample_rate":rate}),
            )
            .unwrap();
    }
    for kind in configured
        .map(|(_, kind, _, _)| kind)
        .into_iter()
        .chain(["slow_user_request"])
    {
        let entries = [(); 3].map(|()| {
            openrust_pulse::Entry::new(timestamp, kind, "42", Some(timestamp))
                .count()
                .max()
                .into()
        });
        store.store(&entries).await.unwrap();
    }

    let target = "/pulse/api/metrics?period=1h";
    let first = app.get(target).await.unwrap();
    first.assert_ok();
    let first = first.json::<serde_json::Value>().unwrap();
    for (card, _, _, rate) in configured {
        assert_eq!(first["sample_rates"][card], rate, "{card}");
        assert_eq!(php_float_cast(&first[card][0]["count"]), 3.0, "{card}");
    }
    for (usage, rate) in [("slow_requests", 0.5), ("jobs", 0.6)] {
        let selected = app.get(&format!("{target}&usage={usage}")).await.unwrap();
        selected.assert_ok();
        let selected = selected.json::<serde_json::Value>().unwrap();
        assert_eq!(selected["sample_rates"]["requests"], rate);
        assert_eq!(php_float_cast(&selected["requests"][0]["count"]), 3.0);
    }
    config
        .set("pulse.recorders.UserRequests.sample_rate", json!(0))
        .unwrap();
    let warm = app.get(target).await.unwrap();
    warm.assert_ok();
    let warm = warm.json::<serde_json::Value>().unwrap();
    assert_eq!(warm["sample_rates"]["requests"], 0.0);
    assert_eq!(warm["requests"], first["requests"]);
    assert_eq!(
        warm["query_timing"]["requests"],
        first["query_timing"]["requests"]
    );
}

#[tokio::test]
async fn dashboard_cards_follow_fixed_orderings_and_usage_limits() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"]["SlowRequests"] = false.into();
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let cards = [
        ("slow_requests", "slow_request", "slow-requests", "count"),
        ("slow_queries", "slow_query", "slow-queries", "count"),
        ("slow_jobs", "slow_job", "slow-jobs", "count"),
        (
            "slow_outgoing_requests",
            "slow_outgoing_request",
            "slow-outgoing-requests",
            "count",
        ),
        ("exceptions", "exception", "exceptions", "latest"),
    ];
    let mut items = Vec::new();
    for (_, kind, _, _) in cards {
        for (key, value) in [("frequent", 100), ("frequent", 200), ("peak", 500)] {
            items.push(
                openrust_pulse::Entry::new(timestamp, kind, key, Some(value))
                    .count()
                    .max()
                    .into(),
            );
        }
    }
    for (kind, range) in [
        ("user_request", 1..=12),
        ("user_job", 21..=32),
        ("slow_user_request", 41..=52),
    ] {
        for user in range {
            let count = user % 20;
            for _ in 0..count {
                items.push(
                    openrust_pulse::Entry::new(timestamp, kind, user.to_string(), None)
                        .count()
                        .into(),
                );
            }
        }
    }
    storage.store(&items).await.unwrap();
    let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    response.assert_ok();
    let default = response.json::<serde_json::Value>().unwrap();
    let override_query = cards
        .iter()
        .map(|(_, _, query, value)| format!("{query}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let response = app
        .get(&format!(
            "/pulse/api/metrics?period=1h&{override_query}&usage=slow_requests"
        ))
        .await
        .unwrap();
    response.assert_ok();
    let selection = response.json::<serde_json::Value>().unwrap();
    for (name, _, _, _) in cards {
        let (default_key, selected_key) = if name == "exceptions" {
            ("frequent", "peak")
        } else {
            ("peak", "frequent")
        };
        assert_eq!(default[name][0]["key"], default_key, "{name}");
        assert_eq!(selection[name][0]["key"], selected_key, "{name}");
    }
    for (data, card, expected) in [
        (&default, "requests", 12),
        (&default, "jobs", 32),
        (&selection, "requests", 52),
    ] {
        let rows = data[card].as_array().unwrap();
        let keys = rows
            .iter()
            .map(|row| row["key"].as_str().unwrap().parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(keys, ((expected - 9)..=expected).rev().collect::<Vec<_>>());
    }
    let response = app
        .get("/pulse/api/metrics?usage=jobs&slow-requests=unknown&exceptions=unknown")
        .await
        .unwrap();
    response.assert_ok();
    let jobs = response.json::<serde_json::Value>().unwrap();
    assert_eq!(jobs["requests"], default["jobs"]);
    assert_eq!(jobs["slow_requests"], default["slow_requests"]);
    assert_eq!(jobs["exceptions"], default["exceptions"]);
    app.get("/pulse/api/metrics?usage=invalid")
        .await
        .unwrap()
        .assert_status(http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dashboard_cache_totals_count_keys_beyond_the_table_limit() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"]["CacheInteractions"] = json!({
                    "enabled": false,
                    "sample_rate": 0.5,
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    response.assert_ok();
    let empty = response.json::<serde_json::Value>().unwrap();
    assert_eq!(
        empty["cache_totals"],
        json!({"hits":0.0,"misses":0.0,"sample_rate":0.5})
    );
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let mut entries = (0..102)
        .map(|key| {
            openrust_pulse::Entry::new(timestamp, "cache_hit", format!("key-{key}"), None)
                .count()
                .into()
        })
        .collect::<Vec<_>>();
    entries.push(
        openrust_pulse::Entry::new(timestamp, "cache_miss", "key-0", None)
            .count()
            .into(),
    );
    storage.store(&entries).await.unwrap();
    let cached = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    cached.assert_ok();
    let cached = cached.json::<serde_json::Value>().unwrap();
    assert_eq!(cached["cache"], empty["cache"]);
    assert_eq!(cached["cache_totals"], empty["cache_totals"]);
    let response = app.get("/pulse/api/metrics?period=6h").await.unwrap();
    response.assert_ok();
    let metrics = response.json::<serde_json::Value>().unwrap();
    assert_eq!(metrics["cache"].as_array().unwrap().len(), 101);
    assert_eq!(
        metrics["cache_totals"],
        json!({"hits":102.0,"misses":1.0,"sample_rate":0.5})
    );
}

#[tokio::test]
async fn native_dashboard_gate_published_view_and_metrics_reject_invalid_periods() {
    use openrust::contracts::foundation::Application as _;
    let mut app = app();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let response = app.get("/pulse").await.unwrap();
    assert_eq!(
        response.status().as_u16(),
        200,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    app.get("/pulse/api/metrics?period=1h")
        .await
        .unwrap()
        .assert_ok();
    app.get("/pulse/api/metrics?period=bad")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::BAD_REQUEST);
    app.app()
        .config()
        .unwrap()
        .set("app.env", json!("production"))
        .unwrap();
    app.get("/pulse")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::FORBIDDEN);
    app.get("/pulse/api/metrics?period=1h")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::FORBIDDEN);
    app.app()
        .make::<openrust::auth::access::GateRegistry>()
        .await
        .unwrap()
        .define_guest("viewPulse", |_, _| {
            Box::pin(async { Ok(openrust::auth::access::Response::allow()) })
        });
    app.get("/pulse").await.unwrap().assert_ok();
    let pulse = app.app().make::<Pulse>().await.unwrap();
    let stylesheet = app.root().join("custom.css");
    std::fs::write(&stylesheet, ".custom{color:red}").unwrap();
    pulse.with_css([
        openrust_pulse::Css::Path(stylesheet.clone()),
        openrust_pulse::Css::Path(stylesheet.clone()),
        openrust_pulse::Css::Html("<style>.html{color:blue}</style>".into()),
    ]);
    assert_eq!(pulse.css().unwrap().matches(".custom").count(), 1);
    let response = app.get("/pulse").await.unwrap();
    response.assert_ok();
    let html = String::from_utf8_lossy(response.body());
    assert!(html.contains(".custom{color:red}"));
    assert!(html.contains(".html{color:blue}"));
    std::fs::remove_file(stylesheet).unwrap();
    assert!(pulse.css().is_err());
    app.get("/pulse")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
    app.artisan(["vendor:publish", "--tag=pulse-dashboard"])
        .await
        .assert_successful();
    std::fs::write(
        app.root()
            .join("resources/views/vendor/pulse/dashboard.html"),
        "<h1>Custom dashboard</h1>",
    )
    .unwrap();
    app.app()
        .make::<openrust::view::Factory>()
        .await
        .unwrap()
        .get_finder()
        .flush();
    app.get("/pulse")
        .await
        .unwrap()
        .assert_ok()
        .assert_content("<h1>Custom dashboard</h1>");
}
#[tokio::test]
async fn native_server_beats_throttle_and_commands_use_configured_cache() {
    use openrust_pulse::recorders::{Memory, Servers};
    let mut app = app();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    let servers = application.make::<Servers>().await.unwrap();
    servers.detect_cpu_using(Some(Arc::new(|| 23)));
    servers.detect_memory_using(Some(Arc::new(|| Memory {
        total: 8192,
        used: 2048,
    })));
    // Application scope is installed by Artisan; its SharedBeat invokes the same sampler.
    app.artisan(["pulse:check", "--once"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let values = storage.values("system", None).await.unwrap();
    assert_eq!(values.len(), 1);
    let Some(openrust::database::Value::Text(value)) = values[0].get("value") else {
        panic!()
    };
    let value: serde_json::Value = serde_json::from_str(value).unwrap();
    assert_eq!(value["cpu"], 23);
    assert_eq!(value["memory_used"], 2048);
    app.artisan(["pulse:work", "--stop-when-empty"])
        .await
        .assert_successful();
    app.artisan(["pulse:restart"]).await.assert_successful();
    app.artisan(["pulse:clear", "--type=cpu", "--force"])
        .await
        .assert_successful();
    assert_eq!(storage.values("system", None).await.unwrap().len(), 1);
    app.artisan(["pulse:purge", "--force"])
        .await
        .assert_successful();
    assert!(storage.values("system", None).await.unwrap().is_empty());
}

struct Identity(u64);
impl openrust::contracts::auth::Authenticatable for Identity {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        self.0.into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
fn observed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::support::facades::{Auth, Cache, Config, Db, Facade, Http, Log};
        Auth::user(request).await?;
        Db::select("select 1 as probe", &[]).await?;
        Cache::get("feature:123").await?;
        Cache::get("zero-width:a").await?;
        Cache::get("secret:skip").await?;
        Cache::get("中文").await?;
        Cache::put("feature:123", &openrust::cache::Value::from("cached"), None).await?;
        Cache::get("feature:123").await?;
        let log = Log::channel(None)?;
        log.log_exception(
            openrust::log::Level::Error,
            "exception attached to an application log",
            &std::fmt::Error,
            &Default::default(),
        )?;
        openrust::support::report(std::io::Error::other("reported exception")).await?;
        openrust::support::report(std::fmt::Error).await?;
        openrust_pulse::facades::Pulse::report("ExplicitPulseReport", Some("probe:1".into()))
            .await
            .unwrap();
        Log::error(
            "user context is not an actual exception",
            &[("exception".into(), json!({"class":"FakeException"}))]
                .into_iter()
                .collect(),
        )?;
        let config = Config::get_facade_root()?;
        let url = config
            .get("probe.url")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap();
        Http::get(&url)
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        if request.uri().query().is_some_and(|q| q.contains("drop=1")) {
            config.set("pulse.recorders.CacheInteractions.sample_rate", json!(0))?;
        }
        Ok(Response::from("observed"))
    })
}
fn recorder_config(
    root: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let mut values = recorder_database_config(root, env)?;
    values.get_mut("pulse").unwrap()["recorders"] = json!({
        "SlowRequests": {"threshold": 0},
        "SlowQueries": {"threshold": 1000000000000_i64, "location": false},
        "SlowOutgoingRequests": {"threshold": 0},
        "CacheInteractions": {
            "ignore": [r"/secret:/A", r"/^\w+$/u"],
            "groups": [
                [r"/feature:\d+\z/A", "feature:*"],
                [r"/feature/", "second"],
                [r"/(?<=zero-width:)(?=a)|(?<=zero-width:)a/", "X"]
            ]
        },
        "Exceptions": {"location": false},
        "UserRequests": {}
    });
    Ok(values)
}

#[tokio::test]
async fn explicit_exception_reports_work_with_a_custom_handler_without_runtime_configuration() {
    use openrust::contracts::{
        container::Container as _,
        debug::{Exception, ExceptionHandler, SharedException},
        events::{Event as EventContract, Listener},
        foundation::Application as _,
    };
    use openrust::{
        events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
        support::facades::Event,
    };
    use openrust_pulse::events::NativeExceptionReported;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ObserveNativeReport(Arc<std::io::Error>, Arc<AtomicUsize>);
    impl Listener for ObserveNativeReport {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventContract,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = event.downcast_ref::<NativeExceptionReported>().unwrap();
                let exception = Exception::Error(&event.exception);
                assert!(std::ptr::eq(
                    exception.downcast_ref::<std::io::Error>().unwrap(),
                    self.0.as_ref()
                ));
                self.1.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            })
        }
    }

    struct CustomHandler;
    impl ExceptionHandler for CustomHandler {
        fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
            Box::pin(async { true })
        }
        fn report<'a>(
            &'a self,
            _: &'a Exception<'_>,
        ) -> HttpFuture<'a, openrust::log::LogResult<()>> {
            Box::pin(async { Ok(()) })
        }
        fn render(&self, _: &Request, _: &Exception<'_>) -> Response {
            Response::from("custom handler")
        }
        fn render_for_console(&self, _: &mut openrust::console::Output, _: &Exception<'_>) {}
    }

    fn report(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            openrust::support::report(std::io::Error::other("custom report")).await?;
            openrust_pulse::facades::Pulse::report("ExplicitPulseReport", Some("probe:1".into()))
                .await
                .unwrap();
            let original = Arc::new(std::io::Error::other("native pulse report"));
            let seen = Arc::new(AtomicUsize::new(0));
            Event::listen(
                NativeExceptionReported::NAME,
                Arc::new(ObserveNativeReport(original.clone(), seen.clone())),
                Delivery::Sync,
            )
            .unwrap();
            let error = SharedException::from_typed_arc(original);
            let manager = openrust_pulse::facades::Pulse::manager().unwrap();
            let returned = manager.report_exception(error.clone()).await;
            assert!(std::ptr::eq(returned, manager.as_ref()));
            openrust_pulse::facades::Pulse::report_exception(error.clone())
                .await
                .unwrap();
            assert_eq!(seen.load(Ordering::SeqCst), 2);
            manager.stop_recording();
            manager.report_exception(error).await;
            manager.start_recording();
            assert_eq!(seen.load(Ordering::SeqCst), 3);
            Ok(Response::from("ok"))
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(recorder_config)
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/report", report)?;
                Ok(())
            }))
    })
    .unwrap();
    app.app()
        .container()
        .instance::<dyn ExceptionHandler>(Arc::new(CustomHandler))
        .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/report").await.unwrap().assert_ok();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let entries = storage
        .connection()
        .table("pulse_entries")
        .r#where("type", "=", "exception")
        .get()
        .await
        .unwrap();
    let count = |class: &str| {
        let key = openrust::database::Value::Text(json!([class, null]).to_string());
        entries
            .iter()
            .filter(|row| row.get("key") == Some(&key))
            .count()
    };
    assert_eq!(entries.len(), 3);
    assert_eq!(count("ExplicitPulseReport"), 1);
    assert_eq!(count(std::any::type_name::<std::io::Error>()), 2);
}

#[tokio::test]
async fn exceptions_group_failed_blade_responses_by_original_view_and_error() {
    use openrust::{
        contracts::{
            debug::{Exception, SharedException},
            foundation::Application as _,
            support::Responsable as _,
        },
        support::facades::View,
        view::{ViewData, ViewError, ViewException, ViewObject, ViewResult, ViewValue},
    };

    #[derive(Debug)]
    struct TemplateFailure;
    impl std::fmt::Display for TemplateFailure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("模板调用失败")
        }
    }
    impl std::error::Error for TemplateFailure {}
    struct TemplateModel;
    impl ViewObject for TemplateModel {
        fn call(&self, _: &str, _: &[ViewValue]) -> ViewResult<ViewValue> {
            Err(ViewError::RenderFailed(Box::new(SharedException::new(
                TemplateFailure,
            ))))
        }
    }

    fn failing_view(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let name = if request.uri().path().ends_with("/outer") {
                "outer"
            } else {
                "broken"
            };
            let data = ViewData::from_iter([(
                "model".into(),
                ViewValue::from_object(Arc::new(TemplateModel)),
            )]);
            let view = View::make(name, data).unwrap();
            let Err(error) = view.to_response(request).await else {
                panic!("预期模板渲染失败");
            };
            assert!(
                ViewException::from_exception(&Exception::Http(&error)).is_some(),
                "{error:?}"
            );
            Err(error)
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"] =
                    json!({"Exceptions": {"location": true}});
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/broken", failing_view)?;
                Route::get("/outer", failing_view)?;
                Ok(())
            }))
    })
    .unwrap();
    let views = app.root().join("resources/views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("broken.blade.html"), "{{ $model->method() }}").unwrap();
    std::fs::write(views.join("outer.blade.html"), "@include('broken')").unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();

    app.get("/api/broken")
        .await
        .unwrap()
        .assert_status(http::StatusCode::INTERNAL_SERVER_ERROR);
    app.get("/api/outer")
        .await
        .unwrap()
        .assert_status(http::StatusCode::INTERNAL_SERVER_ERROR);
    app.app()
        .config()
        .unwrap()
        .set("pulse.recorders.Exceptions.location", json!(false))
        .unwrap();
    app.get("/api/broken")
        .await
        .unwrap()
        .assert_status(http::StatusCode::INTERNAL_SERVER_ERROR);

    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let rows = storage
        .connection()
        .table("pulse_entries")
        .r#where("type", "=", "exception")
        .get()
        .await
        .unwrap();
    let keys: Vec<_> = rows.iter().map(|row| row.get("key").unwrap()).collect();
    let class = std::any::type_name::<TemplateFailure>();
    let path = Some("resources/views/broken.blade.html");
    assert_eq!(keys.len(), 3);
    for (class, location) in [
        (class, path),
        (class, None),
        (std::any::type_name::<ViewException>(), path),
    ] {
        assert!(
            keys.contains(&&openrust::database::Value::Text(
                json!([class, location]).to_string()
            )),
            "{keys:?}"
        );
    }
}

#[tokio::test]
async fn exceptions_keep_concrete_types_and_the_captured_origin_through_reporting() {
    use openrust::contracts::{debug::SharedException, foundation::Application as _};

    #[derive(Debug)]
    struct LocatedFailure;
    impl std::fmt::Display for LocatedFailure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("业务处理失败")
        }
    }
    impl std::error::Error for LocatedFailure {}

    fn report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let line = line!() + 1;
            let error = SharedException::located(LocatedFailure);
            let propagated = error.clone().with_caller();
            assert!(error.callers().is_empty());
            assert_eq!(propagated.callers().len(), 1);
            if request.uri().path().ends_with("/propagated") {
                return Err(propagated.into());
            }
            if request.uri().path().ends_with("/transported-report") {
                openrust::support::report(HttpError::from(propagated)).await?;
                return Ok(Response::from("reported"));
            }
            if request.uri().path().ends_with("/pulse-report") {
                openrust_pulse::facades::Pulse::report_exception(propagated)
                    .await
                    .unwrap();
                return Ok(Response::from("reported"));
            }
            openrust::support::report(propagated).await?;
            openrust::support::report(LocatedFailure).await?;
            Response::json(
                http::StatusCode::OK,
                json!(format!("{}:{line}", file!().replace('\\', "/"))),
            )
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"] =
                    json!({"Exceptions": {"location": true}});
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/reported", report)?;
                Route::get("/propagated", report)?;
                Route::get("/transported-report", report)?;
                Route::get("/pulse-report", report)?;
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();

    let response = app.get("/api/reported").await.unwrap();
    response.assert_ok();
    let location: String = serde_json::from_slice(response.body()).unwrap();
    app.get("/api/propagated")
        .await
        .unwrap()
        .assert_server_error();
    app.get("/api/transported-report")
        .await
        .unwrap()
        .assert_ok();
    app.get("/api/pulse-report").await.unwrap().assert_ok();
    app.app()
        .config()
        .unwrap()
        .set("pulse.recorders.Exceptions.location", json!(false))
        .unwrap();
    app.get("/api/propagated")
        .await
        .unwrap()
        .assert_server_error();

    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let rows = storage
        .connection()
        .table("pulse_entries")
        .r#where("type", "=", "exception")
        .get()
        .await
        .unwrap();
    let class = std::any::type_name::<LocatedFailure>();
    let count = |location: Option<&str>| {
        let key = openrust::database::Value::Text(json!([class, location]).to_string());
        rows.iter()
            .filter(|row| row.get("key") == Some(&key))
            .count()
    };
    assert_eq!(rows.len(), 6, "{rows:?}");
    assert_eq!(count(Some(&location)), 4, "{rows:?}");
    assert_eq!(count(None), 2, "{rows:?}");
}

#[tokio::test]
async fn native_recorders_capture_real_client_sql_cache_and_typed_errors_without_identity_leaks() {
    use openrust::contracts::foundation::Application as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        }
    });
    let mut app = TestApp::create(|b| {
        b.with_config(recorder_config)
            .with_exceptions(|exceptions| {
                exceptions.level::<std::io::Error>(openrust::log::Level::Warning);
                exceptions.report::<std::fmt::Error>(|_| {
                    openrust::foundation::configuration::ReportDecision::Stop
                });
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_routing(Routing::default().api(routes))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let config = app.app().config().unwrap();
    config
        .set(
            "probe.url",
            json!(format!("http://user:secret@{address}/metrics")),
        )
        .unwrap();
    config
        .set(
            "logging",
            json!({"default":"null","channels":{"null":{"driver":"null","level":"error"}}}),
        )
        .unwrap();
    let logged_exceptions = Arc::new(std::sync::Mutex::new(Vec::new()));
    app.app()
        .make::<openrust::log::LogManager>()
        .await
        .unwrap()
        .listen({
            let logged_exceptions = logged_exceptions.clone();
            move |event| {
                if event.exception.is_some() {
                    logged_exceptions
                        .lock()
                        .unwrap()
                        .push(event.message.clone());
                }
                Ok(())
            }
        })
        .unwrap();
    config
        .set("pulse.recorders.SlowQueries.threshold", json!(0))
        .unwrap();
    app.get("/api/observe?user=1").await.unwrap().assert_ok();
    app.get("/api/observe?drop=1").await.unwrap().assert_ok();
    peer.await.unwrap();
    assert_eq!(
        logged_exceptions.lock().unwrap().as_slice(),
        ["exception attached to an application log"; 2],
    );
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let connection = storage.connection();
    let rows = connection
        .table("pulse_entries")
        .r#where("type", "=", "user_request")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("key"),
        Some(&openrust::database::Value::Text("42".into()))
    );
    let exceptions = connection
        .table("pulse_entries")
        .r#where("type", "=", "exception")
        .get()
        .await
        .unwrap();
    let mut exception_keys = BTreeMap::new();
    for row in &exceptions {
        let Some(openrust::database::Value::Text(key)) = row.get("key") else {
            panic!("异常记录缺少键");
        };
        *exception_keys.entry(key.clone()).or_insert(0) += 1;
    }
    assert_eq!(
        exception_keys,
        BTreeMap::from([
            (
                json!([std::any::type_name::<std::io::Error>(), null]).to_string(),
                2
            ),
            (json!(["ExplicitPulseReport", null]).to_string(), 2),
        ]),
    );
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "slow_query")
            .count("*")
            .await
            .unwrap(),
        2
    );
    let rows = connection
        .table("pulse_entries")
        .r#where("type", "=", "slow_outgoing_request")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let Some(openrust::database::Value::Text(key)) = row.get("key") else {
            panic!()
        };
        assert!(!key.contains("user"));
        assert!(!key.contains("secret"));
    }
    let rows = connection
        .table("pulse_entries")
        .where_in("type", vec!["cache_hit".into(), "cache_miss".into()])
        .get()
        .await
        .unwrap();
    let key_count = |key: &str| {
        rows.iter()
            .filter(|row| row.get("key") == Some(&openrust::database::Value::Text(key.into())))
            .count()
    };
    assert_eq!(rows.len(), 3);
    assert_eq!(key_count("feature:*"), 2);
    assert_eq!(key_count("zero-width:XX"), 1);
}

fn outgoing_attempts(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::support::facades::{Config, Facade, Http};
        let config = Config::get_facade_root()?;
        let base = config.get("probe.url").unwrap();
        let base = base.as_str().unwrap();
        let response = Http::pending_request()
            .unwrap()
            .retry(2, std::time::Duration::ZERO)
            .get(&format!("{base}/attempt"))
            .await
            .unwrap();
        assert!(response.successful());
        assert!(Http::get(&format!("{base}/disconnect")).await.is_err());
        let pulse = openrust_pulse::facades::Pulse::get_facade_root()?;
        openrust_pulse::recorders::SlowOutgoingRequests::new(config)
            .record_outgoing(&pulse, "GET", &format!("{base}/manual?keep=yes"), 25)
            .await
            .unwrap();
        Ok(Response::from("ok"))
    })
}

#[tokio::test]
async fn slow_outgoing_records_retry_attempts_failures_and_sanitized_direct_calls() {
    use openrust::contracts::foundation::Application as _;
    use openrust_pulse::recorders::SlowOutgoingRequests;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let original = "https://EXAMPLE.test/item?q=%40";
    assert_eq!(
        SlowOutgoingRequests::normalize_url(original).unwrap(),
        original
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        for status in [Some("500 Internal Server Error"), Some("200 OK"), None] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            if let Some(status) = status {
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        }
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(recorder_config)
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/outgoing", outgoing_attempts)?;
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let config = app.app().config().unwrap();
    config
        .set(
            "probe.url",
            json!(format!("http://client:private@{address}")),
        )
        .unwrap();
    config
        .set(
            "pulse.recorders.SlowOutgoingRequests",
            json!({
                "threshold":0,
                "groups":[[r"#^http://127\.0\.0\.1:\d+/attempt$#", "retry-attempt"]]
            }),
        )
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), app.get("/api/outgoing"))
        .await
        .unwrap()
        .unwrap()
        .assert_ok();
    peer.await.unwrap();
    let store = app.app().make::<DatabaseStorage>().await.unwrap();
    let rows = store
        .connection()
        .table("pulse_entries")
        .r#where("type", "=", "slow_outgoing_request")
        .get()
        .await
        .unwrap();
    let mut keys = rows
        .iter()
        .map(|row| {
            let Some(openrust::database::Value::Text(key)) = row.get("key") else {
                panic!("missing outgoing request key");
            };
            let [method, url] = serde_json::from_str::<[String; 2]>(key).unwrap();
            assert_eq!(method, "GET");
            if url.contains("/manual") {
                assert_eq!(
                    row.get("value"),
                    Some(&openrust::database::Value::Integer(25))
                );
            }
            url
        })
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(
        keys,
        [
            format!("http://{address}/disconnect"),
            format!("http://{address}/manual?keep=yes"),
            "retry-attempt".into(),
            "retry-attempt".into(),
        ]
    );
}

#[tokio::test]
async fn cancelled_nested_ignore_restores_recording_and_rescues_failed_lazy_callbacks() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut app = app();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let pulse = app.app().make::<Pulse>().await.unwrap();
    let errors = Arc::new(AtomicUsize::new(0));
    let count = errors.clone();
    pulse.handle_exceptions_using(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(1),
        pulse.ignore(pulse.ignore(std::future::pending::<()>())),
    )
    .await;
    assert!(cancelled.is_err());
    assert!(pulse.is_recording());
    pulse
        .lazy(|_| Box::pin(async { Err(openrust_pulse::Error::StateUnavailable) }))
        .await;
    pulse.record("custom", "survived", None, None).await.count();
    assert_eq!(pulse.ingest().await, 1);
    assert_eq!(errors.load(Ordering::SeqCst), 1);
    assert!(!pulse.wants_ingesting());
    assert!(pulse.is_recording());
}

struct CustomRecorder;
impl openrust_pulse::recorders::Recorder for CustomRecorder {
    fn name(&self) -> &'static str {
        "CustomRecorder"
    }
    fn listens(&self) -> &'static [&'static str] {
        &["PulseCustomProbe"]
    }
    fn record<'a>(
        &'a self,
        _: &'a dyn openrust::contracts::events::Event,
        pulse: &'a Pulse,
    ) -> openrust_pulse::PulseFuture<'a, ()> {
        Box::pin(async move {
            pulse
                .record("custom_probe", "observed", None, None)
                .await
                .count();
            Ok(())
        })
    }
}
struct CustomProvider;
impl openrust::support::ServiceProvider for CustomProvider {
    fn boot<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            let pulse = app.make::<Pulse>().await?;
            pulse.register(vec![Arc::new(CustomRecorder)])?;
            assert!(
                pulse
                    .recorders()
                    .iter()
                    .any(|r| r.name() == "CustomRecorder")
            );
            assert!(!pulse.recorders().iter().any(|r| r.name() == "SlowQueries"));
            Ok(())
        })
    }
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        openrust::support::facades::Event::dispatch(openrust::events::NamedEvent::new(
            "PulseCustomProbe",
            (),
        ))
        .await?;
        Ok(Response::from("recorded"))
    })
}

fn recording_control_probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust::support::facades::Event;
        use openrust_pulse::facades::Pulse as PulseFacade;

        let pulse = PulseFacade::manager()?;
        pulse.ignore(async { pulse.stop_recording() }).await;
        assert!(pulse.is_recording());
        pulse.stop_recording();
        let result = pulse
            .ignore(async {
                assert!(!pulse.is_recording());
                Event::dispatch(openrust::events::NamedEvent::new("PulseCustomProbe", ())).await?;
                pulse.start_recording();
                pulse.ignore(async { pulse.stop_recording() }).await;
                assert!(pulse.is_recording());
                Event::dispatch(openrust::events::NamedEvent::new("PulseCustomProbe", ())).await
            })
            .await;
        result?;
        assert!(!pulse.is_recording());
        let cancelled = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            pulse.ignore(async {
                pulse.start_recording();
                assert!(pulse.is_recording());
                std::future::pending::<()>().await;
            }),
        )
        .await;
        assert!(cancelled.is_err());
        assert!(!pulse.is_recording());
        Ok(Response::from("recording state restored"))
    })
}

#[tokio::test]
async fn compiled_custom_recorders_register_native_event_listeners_in_each_application() {
    let mut fixture = TestApp::create(|b| {
        b.with_config(recorder_database_config)
            .with_providers(vec![
                Box::new(PulseServiceProvider),
                Box::new(CustomProvider),
            ])
            .with_routing(Routing::default().api(|| {
                Route::get("/probe", probe)?;
                Route::get("/recording-control", recording_control_probe)?;
                Ok(())
            }))
    })
    .unwrap();
    fixture
        .artisan(["migrate", "--force"])
        .await
        .assert_successful();
    fixture.get("/api/probe").await.unwrap().assert_ok();
    fixture
        .get("/api/recording-control")
        .await
        .unwrap()
        .assert_ok();
    let storage = fixture.app().make::<DatabaseStorage>().await.unwrap();
    assert_eq!(
        storage
            .connection()
            .table("pulse_entries")
            .r#where("type", "=", "custom_probe")
            .count("*")
            .await
            .unwrap(),
        2
    );
    let mut other = app();
    other
        .artisan(["migrate", "--force"])
        .await
        .assert_successful();
    assert!(
        !other
            .app()
            .make::<Pulse>()
            .await
            .unwrap()
            .recorders()
            .iter()
            .any(|r| r.name() == "CustomRecorder")
    );
}

struct ReadOnlyUsers;
impl openrust_pulse::ResolvesUsers for ReadOnlyUsers {
    fn load<'a>(&'a self, _: &'a [String]) -> openrust_pulse::PulseFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn find(&self, key: &str) -> openrust_pulse::User {
        openrust_pulse::User {
            name: key.into(),
            extra: None,
            avatar: None,
        }
    }
}

fn usage_user_config(
    root: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let mut values = recorder_database_config(root, env)?;
    let auth = values.get_mut("auth").unwrap();
    auth["guards"]["web"]["provider"] = json!("pulse-users");
    auth["providers"] = json!({"pulse-users":{"driver":"database","table":"pulse_test_users"}});
    Ok(values)
}

#[tokio::test]
async fn usage_resolves_fixed_user_details_and_gravatar_fallbacks_from_the_native_provider() {
    use openrust::database::{DatabaseManager, Value as DbValue};

    let mut app = TestApp::create(|b| {
        b.with_config(usage_user_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    let connection = application
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .get_schema_builder()
        .create("pulse_test_users", |table| {
            table.string("id", 100).primary();
            table.string("name", 100);
            table.string("email", 150).nullable();
            table.string("avatar", 150).nullable();
        })
        .await
        .unwrap();
    let unicode_email = "\t \u{a0}ÉXAMPLE@EXAMPLE.TEST \t";
    let avatar_url = "https://profile.example.test/face.png";
    let users = [
        ("missing-email", "No mail", None, None),
        ("empty-email", "Empty", Some(""), None),
        ("zero-email", "Zero", Some("0"), None),
        ("unicode-email", "Unicode", Some(unicode_email), None),
        ("explicit-avatar", "Explicit", None, Some(avatar_url)),
        (
            "empty-avatar",
            "Blank",
            Some("person@example.test"),
            Some(""),
        ),
    ];
    for (id, name, email, avatar) in users {
        connection
            .table("pulse_test_users")
            .insert(&[BTreeMap::from([
                ("id".into(), id.into()),
                ("name".into(), name.into()),
                ("email".into(), email.map_or(DbValue::Null, DbValue::from)),
                ("avatar".into(), avatar.map_or(DbValue::Null, DbValue::from)),
            ])])
            .await
            .unwrap();
    }
    let pulse = application.make::<Pulse>().await.unwrap();
    for id in users
        .map(|(id, _, _, _)| id)
        .into_iter()
        .chain(["missing-user"])
    {
        pulse.record("user_request", id, None, None).await.count();
    }
    assert_eq!(pulse.ingest().await, 7);
    let response = app.get("/pulse/api/metrics?cards=requests").await.unwrap();
    response.assert_ok();
    let data: serde_json::Value = response.json().unwrap();
    let rows = data["requests"].as_array().unwrap();
    assert_eq!(rows.len(), 7);
    let default_avatar = "https://gravatar.com/avatar?d=mp";
    let unicode_avatar = "https://gravatar.com/avatar/5c1454e4b1681442b48245b957115af11f322bec92872d1aadd60e108603c313?d=mp";
    for (key, name, extra, avatar) in [
        ("missing-email", "No mail", "", default_avatar),
        ("empty-email", "Empty", "", default_avatar),
        ("zero-email", "Zero", "0", default_avatar),
        ("unicode-email", "Unicode", unicode_email, unicode_avatar),
        ("explicit-avatar", "Explicit", "", avatar_url),
        ("empty-avatar", "Blank", "person@example.test", ""),
        ("missing-user", "ID: missing-user", "", default_avatar),
    ] {
        let row = rows.iter().find(|row| row["key"] == key).unwrap();
        assert_eq!(
            row["user"],
            json!({"name":name,"extra":extra,"avatar":avatar}),
            "{key}"
        );
    }
}

#[tokio::test]
async fn usage_loads_eloquent_users_in_one_query() {
    use openrust::{
        auth::{AuthIdentifier, AuthManager, AuthResult},
        contracts::auth::Authenticatable,
        database::{
            DatabaseManager, Value as DbValue,
            eloquent::{Model, ModelMetadata, ModelState},
        },
    };

    struct UsageUser(ModelState);
    impl Model for UsageUser {
        fn metadata() -> &'static ModelMetadata {
            static META: ModelMetadata =
                ModelMetadata::new("pulse_eloquent_users").timestamps(None, None);
            &META
        }
        fn from_state(state: ModelState) -> Self {
            Self(state)
        }
        fn state(&self) -> &ModelState {
            &self.0
        }
        fn state_mut(&mut self) -> &mut ModelState {
            &mut self.0
        }
    }
    impl Authenticatable for UsageUser {
        fn get_auth_identifier_name(&self) -> &str {
            "id"
        }
        fn get_auth_identifier(&self) -> AuthIdentifier {
            self.get_key().unwrap().try_into().unwrap()
        }
        fn get_auth_password_name(&self) -> &str {
            "password"
        }
        fn get_auth_password(&self) -> Option<&str> {
            None
        }
        fn get_remember_token(&self) -> Option<&str> {
            None
        }
        fn set_remember_token(&mut self, _: Option<String>) {}
        fn get_remember_token_name(&self) -> &str {
            "remember_token"
        }
        fn get_auth_attribute(&self, attribute: &str) -> AuthResult<Option<DbValue>> {
            Ok(self.get_attribute(attribute).cloned())
        }
    }

    let mut app = TestApp::create(|b| {
        b.with_config(|root, env| {
            let mut values = usage_user_config(root, env)?;
            values.get_mut("auth").unwrap()["providers"]["pulse-users"] =
                json!({"driver":"eloquent","model":"PulseUsageUser"});
            Ok(values)
        })
        .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    application
        .make::<AuthManager>()
        .await
        .unwrap()
        .register_model::<UsageUser>("PulseUsageUser");
    let connection = application
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .get_schema_builder()
        .create("pulse_eloquent_users", |table| {
            table.big_integer("id").primary();
            table.string("name", 100);
        })
        .await
        .unwrap();
    connection
        .table("pulse_eloquent_users")
        .insert(&[
            BTreeMap::from([("id".into(), 1.into()), ("name".into(), "First".into())]),
            BTreeMap::from([("id".into(), 2.into()), ("name".into(), "Second".into())]),
        ])
        .await
        .unwrap();
    let pulse = application.make::<Pulse>().await.unwrap();
    for id in ["1", "01", "2", "404"] {
        pulse.record("user_request", id, None, None).await.count();
    }
    assert_eq!(pulse.ingest().await, 4);
    connection.enable_query_log();
    let response = app.get("/pulse/api/metrics?cards=requests").await.unwrap();
    response.assert_ok();
    let queries: Vec<_> = connection
        .get_query_log()
        .into_iter()
        .filter(|query| query.query.contains("pulse_eloquent_users"))
        .collect();
    assert_eq!(queries.len(), 1, "expected one Eloquent user query");
    assert!(queries[0].query.contains(" in "));
    let data: serde_json::Value = response.json().unwrap();
    let rows = data["requests"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for (key, expected) in [
        ("1", "First"),
        ("01", "First"),
        ("2", "Second"),
        ("404", "ID: 404"),
    ] {
        let row = rows.iter().find(|row| row["key"] == key).unwrap();
        assert_eq!(row["user"]["name"], expected, "{key}");
    }
}

#[tokio::test]
async fn public_rescue_and_user_resolver_errors_and_documented_metrics_are_observable() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut app = app();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let pulse = app.app().make::<Pulse>().await.unwrap();
    let errors = Arc::new(AtomicUsize::new(0));
    let observed = errors.clone();
    pulse.handle_exceptions_using(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(pulse.rescue(async { Ok(42) }).await, Some(42));
    assert_eq!(
        pulse
            .rescue(async { Err::<(), _>(openrust_pulse::Error::StateUnavailable) })
            .await,
        None
    );
    assert_eq!(errors.load(Ordering::SeqCst), 1);
    pulse
        .user(|_| openrust_pulse::User {
            name: "custom".into(),
            extra: None,
            avatar: None,
        })
        .unwrap();
    let storage = pulse.storage.clone();
    let custom = Pulse::new(
        storage.clone(),
        Arc::new(openrust_pulse::ingests::StorageIngest(storage)),
    )
    .with_user_resolver(Arc::new(ReadOnlyUsers));
    assert!(
        custom
            .user(|_| unreachable!())
            .err()
            .unwrap()
            .to_string()
            .contains("does not support")
    );
    assert_eq!(
        custom
            .resolve_users(&["42".into()])
            .await
            .unwrap()
            .find("42")
            .name,
        "42"
    );
    app.get("/api/metric-example").await.unwrap().assert_ok();
}
fn metric_example(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust_pulse::facades::Pulse;
        let run = async {
            Pulse::record("orders", "completed", Some(120), None)
                .await?
                .count()
                .sum()
                .max()
                .avg();
            Pulse::set("deployment", "version", "v1", None).await?;
            Pulse::ingest().await?;
            let totals = Pulse::aggregate_total(&["orders".into()], "sum", 3600).await?;
            assert_eq!(totals["orders"], Some(120.0));
            Ok::<_, openrust_pulse::Error>(())
        };
        run.await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::from("metrics example accepted"))
    })
}

#[tokio::test]
async fn legacy_bulk_users_remain_application_scoped_and_reject_field_overrides() {
    let mut application = app();
    application.boot().await.unwrap();
    let pulse = application.app().make::<Pulse>().await.unwrap();
    pulse.users(|keys| {
        Ok(keys
            .iter()
            .map(|key| {
                (
                    key.clone(),
                    openrust_pulse::User {
                        name: format!("Bulk {key}"),
                        extra: None,
                        avatar: None,
                    },
                )
            })
            .collect())
    });
    let users = pulse.resolve_users(&["42".into()]).await.unwrap();
    assert_eq!(users.find("42").name, "Bulk 42");
    let missing = users.find("43");
    assert_eq!(missing.name, "ID: 43");
    assert_eq!(missing.extra.as_deref(), Some(""));
    assert_eq!(
        missing.avatar.as_deref(),
        Some("https://gravatar.com/avatar?d=mp")
    );
    assert!(pulse.user(|_| unreachable!()).is_err());
    assert_eq!(users.find("42").name, "Bulk 42");
    let mut other = app();
    other.boot().await.unwrap();
    let other = other.app().make::<Pulse>().await.unwrap();
    assert!(
        other
            .user(|_| openrust_pulse::User {
                name: "other".into(),
                extra: None,
                avatar: None
            })
            .is_ok()
    );
}

#[tokio::test]
async fn legacy_usage_resolves_email_separately_from_explicit_display_fields() {
    use openrust_pulse::User;

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(recorder_database_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let pulse = app.app().make::<Pulse>().await.unwrap();
    let email = " \tMIXED@Example.Test\t";
    pulse.users(move |keys| {
        Ok(keys
            .iter()
            .filter_map(|key| {
                let (name, email, extra, avatar) = match key.as_str() {
                    "1" => ("Email only", email, None, None),
                    "2" => ("Extra override", email, Some("Operations"), None),
                    "3" => ("Blank override", email, Some(""), Some("")),
                    "4" => ("Zero email", "0", None, None),
                    _ => return None,
                };
                Some((
                    key.clone(),
                    User::from_profile(
                        name,
                        Some(email.into()),
                        extra.map(str::to_owned),
                        avatar.map(str::to_owned),
                    ),
                ))
            })
            .collect())
    });
    for id in ["1", "2", "3", "4", "5"] {
        pulse.record("user_request", id, None, None).await.count();
    }
    assert_eq!(pulse.ingest().await, 5);
    let response = app.get("/pulse/api/metrics?cards=requests").await.unwrap();
    response.assert_ok();
    let data: serde_json::Value = response.json().unwrap();
    let rows = data["requests"].as_array().unwrap();
    let email_avatar = "https://gravatar.com/avatar/b5c06a7db4d01a9f46b2c98ba9fb67c4d100663d8f20f44930f7e04fd13c75b5?d=mp";
    let default_avatar = "https://gravatar.com/avatar?d=mp";
    for (key, name, extra, avatar) in [
        ("1", "Email only", email, email_avatar),
        ("2", "Extra override", "Operations", email_avatar),
        ("3", "Blank override", "", ""),
        ("4", "Zero email", "0", default_avatar),
        ("5", "ID: 5", "", default_avatar),
    ] {
        let row = rows.iter().find(|row| row["key"] == key).unwrap();
        assert_eq!(
            row["user"],
            json!({"name":name,"extra":extra,"avatar":avatar})
        );
    }
}

#[tokio::test]
async fn dashboard_server_cache_holds_matching_heartbeats_and_graphs_until_refresh() {
    use openrust::contracts::foundation::Application as _;

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(dashboard_cache_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let now = jiff::Timestamp::now().as_second();
    let items = |timestamp, key, hostname, cpu| {
        [
            openrust_pulse::Value::new(
                timestamp,
                "system",
                key,
                json!({"name":hostname,"cpu":cpu,"memory_used":512,"memory_total":1024,"storage":[]})
                    .to_string(),
            )
            .into(),
            openrust_pulse::Entry::new(timestamp, "cpu", key, Some(cpu))
                .avg()
                .only_buckets()
                .into(),
        ]
    };
    let target = "/pulse/api/metrics?period=1h";
    storage
        .store(&items(now - 26, "node-a", "Alpha", 10))
        .await
        .unwrap();
    let first = app.get(target).await.unwrap();
    first.assert_ok();
    let first = first.json::<serde_json::Value>().unwrap();
    assert_eq!(first["servers"][0]["recently_reported"], true);
    storage
        .store(&items(now, "node-z", "Zulu", 70))
        .await
        .unwrap();

    let cached = app.get(target).await.unwrap();
    cached.assert_ok();
    let cached = cached.json::<serde_json::Value>().unwrap();
    assert_eq!(cached["servers"], first["servers"]);
    assert_eq!(cached["server_graphs"], first["server_graphs"]);
    let separate = app.get("/pulse/api/metrics?period=6h").await.unwrap();
    separate.assert_ok();
    let separate = separate.json::<serde_json::Value>().unwrap();
    assert_eq!(separate["servers"].as_array().unwrap().len(), 2);
    assert!(separate["server_graphs"]["node-z"]["cpu"].is_object());

    tokio::time::sleep(std::time::Duration::from_millis(5200)).await;
    let fresh = app.get(target).await.unwrap();
    fresh.assert_ok();
    let fresh = fresh.json::<serde_json::Value>().unwrap();
    assert_eq!(fresh["servers"][0]["recently_reported"], false);
    assert_eq!(fresh["servers"][1]["key"], "node-z");
    assert!(fresh["server_graphs"]["node-z"]["cpu"].is_object());
    app.app()
        .config()
        .unwrap()
        .set("pulse.dashboard.servers.sort_direction", json!("desc"))
        .unwrap();
    let sorted = app.get(target).await.unwrap();
    sorted.assert_ok();
    assert_eq!(
        sorted.json::<serde_json::Value>().unwrap()["servers"][0]["key"],
        "node-z"
    );
}

#[tokio::test]
async fn dashboard_server_history_returns_average_series_and_heartbeat_state() {
    use openrust::contracts::foundation::Application as _;
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(recorder_database_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let now = jiff::Timestamp::now().as_second();
    let items = [
        openrust_pulse::Value::new(
            now,
            "system",
            "node-z",
            r#"{"name":"Zulu","cpu":33,"memory_used":512,"memory_total":1024,"storage":[]}"#,
        )
        .into(),
        openrust_pulse::Value::new(
            now - 31,
            "system",
            "node-a",
            r#"{"name":"Alpha","cpu":15,"memory_used":256,"memory_total":1024,"storage":[]}"#,
        )
        .into(),
        openrust_pulse::Entry::new(now, "cpu", "node-z", Some(13))
            .avg()
            .only_buckets()
            .into(),
        openrust_pulse::Entry::new(now, "cpu", "node-z", Some(33))
            .avg()
            .only_buckets()
            .into(),
        openrust_pulse::Entry::new(now, "memory", "node-z", Some(256))
            .avg()
            .only_buckets()
            .into(),
        openrust_pulse::Entry::new(now, "memory", "node-z", Some(512))
            .avg()
            .only_buckets()
            .into(),
    ];
    storage.store(&items).await.unwrap();
    let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    response.assert_ok();
    let data = response.json::<serde_json::Value>().unwrap();
    for (metric, expected) in [("cpu", 23.0), ("memory", 384.0)] {
        let readings = data["server_graphs"]["node-z"][metric]
            .as_object()
            .unwrap_or_else(|| panic!("缺少 node-z {metric} 历史，服务状态为 {}", data["servers"]));
        assert_eq!(readings.len(), 60);
        assert_eq!(
            readings
                .values()
                .filter_map(|v| v.as_f64())
                .collect::<Vec<_>>(),
            [expected]
        );
    }
    let rows = data["servers"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["key"], "node-a");
    assert_eq!(rows[0]["recently_reported"], false);
    assert_eq!(rows[1]["key"], "node-z");
    assert_eq!(rows[1]["timestamp"], now);
    assert_eq!(rows[1]["recently_reported"], true);
    assert!(data["server_graphs"]["node-a"].is_null());

    let config = app.app().config().unwrap();
    config
        .set("pulse.dashboard.servers.sort_by", json!("cpu_current"))
        .unwrap();
    config
        .set("pulse.dashboard.servers.sort_direction", json!("desc"))
        .unwrap();
    let response = app.get("/pulse/api/metrics").await.unwrap();
    response.assert_ok();
    assert_eq!(
        response.json::<serde_json::Value>().unwrap()["servers"][0]["key"],
        "node-z"
    );

    for (limit, expected) in [
        (json!(15), 1),
        (json!("15"), 1),
        (json!("15 seconds"), 1),
        (json!("1 minute, 5 seconds"), 2),
        (json!("1 minute and 5 seconds"), 2),
        (json!("1 month"), 2),
        (json!("2 hours ago"), 0),
    ] {
        config
            .set("pulse.dashboard.servers.ignore_after", limit)
            .unwrap();
        let response = app.get("/pulse/api/metrics").await.unwrap();
        response.assert_ok();
        let data = response.json::<serde_json::Value>().unwrap();
        assert_eq!(data["servers"].as_array().unwrap().len(), expected);
        if expected == 0 {
            assert!(data["server_graphs"].as_object().unwrap().is_empty());
        } else {
            assert_eq!(data["servers"][0]["key"], "node-z");
        }
    }
}

#[tokio::test]
async fn dashboard_servers_sort_numeric_names_collections_and_nested_fields() {
    use openrust::contracts::foundation::Application as _;
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(recorder_database_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let mut entries = Vec::new();
    for (key, name, cpu, used, total) in [
        ("node-10", "10", 25, 30, 80),
        ("node-2", "2", 50, 100, 200),
        ("node-alpha", "Alpha", 80, 40, 90),
    ] {
        let snapshot = json!({
            "name": name, "cpu": cpu, "memory_used": 32, "memory_total": 64,
            "storage": [{"directory": "/", "total": total, "used": used}],
        });
        entries.push(
            openrust_pulse::Value::new(timestamp, "system", key, snapshot.to_string()).into(),
        );
        entries.push(
            openrust_pulse::Entry::new(timestamp, "cpu", key, Some(cpu))
                .avg()
                .only_buckets()
                .into(),
        );
    }
    storage.store(&entries).await.unwrap();
    let original: Vec<_> = storage
        .values("system", None)
        .await
        .unwrap()
        .iter()
        .map(|row| match row.get("key").unwrap() {
            openrust::database::Value::Text(key) => key.clone(),
            openrust::database::Value::Bytes(key) => String::from_utf8(key.clone()).unwrap(),
            key => panic!("无法读取服务器键 {key:?}"),
        })
        .collect();
    let config = app.app().config().unwrap();
    let by_name = ["node-2", "node-10", "node-alpha"];
    let by_cpu = ["node-10", "node-2", "node-alpha"];
    let by_disk = ["node-10", "node-alpha", "node-2"];
    for (field, asc) in [
        ("name", by_name.as_slice()),
        ("cpu", by_cpu.as_slice()),
        ("storage", by_disk.as_slice()),
        ("storage.0.used", by_disk.as_slice()),
    ] {
        config
            .set("pulse.dashboard.servers.sort_by", json!(field))
            .unwrap();
        for direction in ["asc", "desc"] {
            config
                .set("pulse.dashboard.servers.sort_direction", json!(direction))
                .unwrap();
            let mut expected = asc.to_vec();
            if direction == "desc" {
                expected.reverse();
            }
            let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
            response.assert_ok();
            let data = response.json::<serde_json::Value>().unwrap();
            let keys: Vec<_> = data["servers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["key"].as_str().unwrap())
                .collect();
            assert_eq!(keys, expected, "{field} {direction}");
        }
    }
    config
        .set("pulse.dashboard.servers.sort_by", json!("unknown"))
        .unwrap();
    let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    response.assert_ok();
    let data = response.json::<serde_json::Value>().unwrap();
    let keys: Vec<_> = data["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, original);
}

#[tokio::test]
async fn dashboard_servers_follow_upstream_order_for_cyclic_name_comparisons() {
    use openrust::contracts::foundation::Application as _;

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(recorder_database_config)
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let timestamp = jiff::Timestamp::now().as_second();
    let values = ["10", "2", "1x"].map(|name| {
        let snapshot = json!({
            "name": name, "cpu": 25, "memory_used": 32, "memory_total": 64, "storage": [],
        });
        openrust_pulse::Value::new(timestamp, "system", name, snapshot.to_string()).into()
    });
    storage.store(&values).await.unwrap();

    let config = app.app().config().unwrap();
    let mut actual = Vec::new();
    for (field, direction) in [("unknown", "asc"), ("name", "asc"), ("name", "desc")] {
        config
            .set("pulse.dashboard.servers.sort_by", json!(field))
            .unwrap();
        config
            .set("pulse.dashboard.servers.sort_direction", json!(direction))
            .unwrap();
        let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
        response.assert_ok();
        let result = response.json::<serde_json::Value>().unwrap();
        let keys = result["servers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["key"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        actual.push(keys);
    }
    let original: Vec<_> = actual[0].iter().map(String::as_str).collect();
    let reverse: Vec<_> = original.iter().rev().copied().collect();
    // 固定 PHP 在这三组输入上保留升序，在另外三组上反转升序。
    if matches!(
        original.as_slice(),
        ["10", "1x", "2"] | ["2", "10", "1x"] | ["1x", "2", "10"]
    ) {
        assert_eq!(actual[1], original, "asc");
        assert_eq!(actual[2], reverse, "desc");
    } else {
        assert_eq!(actual[1], reverse, "asc");
        assert_eq!(actual[2], original, "desc");
    }
}
