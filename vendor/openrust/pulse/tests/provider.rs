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
            .with_routing(Routing::default().api(|| Route::get("/terminal", failed_after_defer)))
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
async fn queued_artisan_recorders_filter_by_display_name() {
    use openrust::{
        database::Value as DbValue,
        queue::Payload,
        support::facades::{Artisan, Auth},
    };

    fn enqueue(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::user(request).await?;
            for name in ["pulse-probe:ignored", "pulse-probe:kept"] {
                Artisan::queue(name, &[]).dispatch().await.unwrap();
            }
            Ok(Response::from("queued"))
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = recorder_database_config(root, env)?;
                values.insert("queue".into(), json!({
                    "default":"database",
                    "connections":{"database":{"driver":"database","table":"jobs","retry_after":90}},
                    "failed":{"driver":"null"}
                }));
                let ignore = json!(["/^pulse-probe:ignored$/"]);
                values.get_mut("pulse").unwrap()["recorders"] = json!({
                    "Queues":{"ignore":ignore},
                    "UserJobs":{"ignore":ignore},
                    "SlowJobs":{"ignore":ignore,"threshold":0}
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider), Box::new(AuthProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/queue-commands", enqueue)
            }).commands(|| {
                Artisan::command("pulse-probe:ignored", |_| {})?;
                Artisan::command("pulse-probe:kept", |_| {})?;
                Ok(())
            }))
    }).unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let connection = storage.connection();
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

    app.get("/api/queue-commands?user=1")
        .await
        .unwrap()
        .assert_ok();
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

    app.artisan([
        "queue:work",
        "database",
        "--stop-when-empty",
        "--sleep=0",
        "--memory=0",
    ])
    .await
    .assert_successful();
    assert_eq!(connection.table("jobs").count("*").await.unwrap(), 0);
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
    let expected = response.json::<String>().unwrap();
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
        Cache::put("feature:123", &openrust::cache::Value::from("cached"), None).await?;
        Cache::get("feature:123").await?;
        let log = Log::channel(None)?;
        log.log_exception(
            openrust::log::Level::Error,
            "typed error",
            &std::io::Error::other("expected"),
            &Default::default(),
        )?;
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
    let mut values = config(root, env)?;
    values.get_mut("pulse").unwrap()["recorders"] = json!({"SlowRequests":{"threshold":0},"SlowQueries":{"threshold":1000000000000_i64,"location":false},"SlowOutgoingRequests":{"threshold":0},"CacheInteractions":{"groups":[[r"/^feature:\d+$/","feature:*"]]},"Exceptions":{"location":false},"UserRequests":{}});
    Ok(values)
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
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        )
        .unwrap();
    config.set("pulse.recorders",json!({"SlowRequests":{"threshold":0},"SlowQueries":{"threshold":0,"location":false},"SlowOutgoingRequests":{"threshold":0},"CacheInteractions":{"groups":[[r"/^feature:\d+$/","feature:*"]]},"Exceptions":{"location":false},"UserRequests":{}})).unwrap();
    app.get("/api/observe?user=1").await.unwrap().assert_ok();
    app.get("/api/observe?drop=1").await.unwrap().assert_ok();
    peer.await.unwrap();
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
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "exception")
            .count("*")
            .await
            .unwrap(),
        2
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
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(
            row.get("key"),
            Some(&openrust::database::Value::Text("feature:*".into()))
        );
    }
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
            .with_routing(Routing::default().api(|| Route::get("/outgoing", outgoing_attempts)))
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
#[tokio::test]
async fn compiled_custom_recorders_register_native_event_listeners_in_each_application() {
    let mut fixture = TestApp::create(|b| {
        b.with_config(config)
            .with_providers(vec![
                Box::new(PulseServiceProvider),
                Box::new(CustomProvider),
            ])
            .with_routing(Routing::default().api(|| {
                Route::get("/probe", probe)?;
                Ok(())
            }))
    })
    .unwrap();
    fixture
        .artisan(["migrate", "--force"])
        .await
        .assert_successful();
    fixture.get("/api/probe").await.unwrap().assert_ok();
    let storage = fixture.app().make::<DatabaseStorage>().await.unwrap();
    assert_eq!(
        storage
            .connection()
            .table("pulse_entries")
            .r#where("type", "=", "custom_probe")
            .count("*")
            .await
            .unwrap(),
        1
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
    assert_eq!(users.find("43").name, "ID: 43");
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
