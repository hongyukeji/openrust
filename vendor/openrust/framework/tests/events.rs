use std::{
    net::SocketAddr,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _,
        events::{
            Dispatcher as _, Event as Payload, Listener, ShouldBeDiscovered,
            ShouldDispatchAfterCommit, Subscriber,
        },
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{DatabaseError, DatabaseManager, Db, Value},
    events::{
        Delivery, DispatchResult, Dispatcher, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse,
    },
    foundation::{Application, configuration::Routing, events::DiscoverEvents, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::Event,
    testing::TestApp,
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Notice {
    name: &'static str,
    note: &'static str,
    observed: Arc<Mutex<Vec<String>>>,
}

impl Notice {
    fn new(name: &'static str, note: &'static str, observed: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            note,
            observed: observed.clone(),
        }
    }
}

impl Payload for Notice {
    fn name(&self) -> &str {
        self.name
    }

    fn interfaces(&self) -> &[&str] {
        &["contracts.notice", "contracts.delivery"]
    }
}

impl ShouldDispatchAfterCommit for Notice {}

struct Record(&'static str);

impl Listener for Record {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Payload,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<Notice>()
                .ok_or(EventError::InvalidPayload)?;
            if self.0 == "after" {
                assert!(context.transaction().is_none());
                let connection = Db::connection(None).map_err(EventError::Transaction)?;
                let count = connection
                    .scalar("select count(*) from event_records", &[])
                    .await
                    .map_err(EventError::Transaction)?;
                assert_eq!(count, Value::Integer(1));
            }
            event
                .observed
                .lock()
                .unwrap()
                .push(format!("{}:{}", self.0, event.note));
            Ok(match self.0 {
                "stop" => Some(ListenerResponse::Stop),
                "answer" => Some(ListenerResponse::Value(Box::new(42_u32))),
                _ => None,
            })
        })
    }
}

fn register(registry: &mut ListenerRegistry) -> EventResult<()> {
    for (name, label, delivery) in [
        ("order.*", "wild", Delivery::Sync),
        ("order.ready", "first", Delivery::Sync),
        ("order.ready", "stop", Delivery::Sync),
        ("order.ready", "skipped", Delivery::Sync),
        ("order.answer", "answer", Delivery::Sync),
        ("order.answer", "skipped", Delivery::Sync),
        ("order.completed", "after", Delivery::AfterCommit),
        ("order.completed", "now", Delivery::Sync),
        ("order.whole", "whole", Delivery::Sync),
    ] {
        registry.listen(name, Arc::new(Record(label)), delivery)?;
    }
    Ok(())
}

struct Extra;
impl Subscriber for Extra {
    fn subscribe(&self, registry: &mut ListenerRegistry) -> EventResult<()> {
        registry.listen("notice.*", Arc::new(Record("subscriber")), Delivery::Sync)
    }
}

fn run(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let result = Event::dispatch(Notice::new("order.ready", "stop", &observed)).await?;
        let DispatchResult::Completed(responses) = result else {
            unreachable!()
        };
        assert_eq!(responses.len(), 1);
        let result = Event::until(Notice::new("order.answer", "until", &observed)).await?;
        let Some(ListenerResponse::Value(answer)) = result else {
            unreachable!()
        };
        assert_eq!(answer.downcast_ref::<u32>(), Some(&42));

        let db = Db::connection(None)?;
        let mut transaction = db.begin_transaction().await?;
        transaction
            .insert("insert into event_records (id) values (1)", &[])
            .await?;
        Event::dispatch_in_transaction(
            Notice::new("order.completed", "outer", &observed),
            &mut transaction,
        )
        .await?;
        transaction.begin_transaction().await?;
        let result = Event::dispatch_after_commit(
            Notice::new("order.whole", "kept", &observed),
            &mut transaction,
        )
        .await?;
        assert!(matches!(result, DispatchResult::Deferred));
        transaction.commit().await?;

        transaction.begin_transaction().await?;
        transaction.begin_transaction().await?;
        Event::dispatch_after_commit(
            Notice::new("order.whole", "discarded", &observed),
            &mut transaction,
        )
        .await?;
        transaction.commit().await?;
        transaction.roll_back(Some(1)).await?;
        assert_eq!(
            &*observed.lock().unwrap(),
            &[
                "first:stop",
                "stop:stop",
                "answer:until",
                "now:outer",
                "wild:outer"
            ]
        );
        transaction.commit().await?;

        let mut rolled_back = db.begin_transaction().await?;
        rolled_back
            .insert("insert into event_records (id) values (3)", &[])
            .await?;
        Event::dispatch_after_commit(
            Notice::new("order.whole", "rolled back", &observed),
            &mut rolled_back,
        )
        .await?;
        rolled_back.roll_back(None).await?;
        assert_eq!(
            db.scalar("select count(*) from event_records", &[]).await?,
            Value::Integer(1),
        );

        let mut discarded = db.begin_transaction().await?;
        Event::dispatch_after_commit(
            Notice::new("order.whole", "dropped", &observed),
            &mut discarded,
        )
        .await?;
        drop(discarded);
        Event::dispatch(Notice::new("notice.extra", "registered", &observed)).await?;
        let result = observed.lock().unwrap().clone();
        Response::json(StatusCode::OK, result)
    })
}

fn isolated(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let observed = Arc::new(Mutex::new(Vec::new()));
        Event::dispatch(Notice::new("order.ready", "isolated", &observed)).await?;
        let result = observed.lock().unwrap().clone();
        Response::json(StatusCode::OK, result)
    })
}

fn deferred(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let observed = Arc::new(Mutex::new(Vec::new()));
        Event::defer(Some(&["order.whole"]), async {
            assert!(matches!(
                Event::dispatch(Notice::new("order.whole", "first", &observed)).await?,
                DispatchResult::Deferred
            ));
            Event::dispatch(Notice::new("order.answer", "immediate", &observed)).await?;
            assert_eq!(
                *observed.lock().unwrap(),
                ["answer:immediate", "skipped:immediate", "wild:immediate"]
            );
            Event::defer(None, async {
                Event::dispatch(Notice::new("order.whole", "nested", &observed)).await?;
                Ok(())
            })
            .await?;
            Event::dispatch(Notice::new("order.whole", "last", &observed)).await?;
            Ok(())
        })
        .await?;
        let failure: EventResult<()> = Event::defer(None, async {
            Event::dispatch(Notice::new("order.whole", "discarded", &observed)).await?;
            Err(EventError::InvalidPayload)
        })
        .await;
        assert!(matches!(failure, Err(EventError::InvalidPayload)));
        let result = observed.lock().unwrap().clone();
        Response::json(StatusCode::OK, result)
    })
}

fn interface_listeners(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let observed = Arc::new(Mutex::new(Vec::new()));
        Event::dispatch(Notice::new("interface.ready", "normal", &observed)).await?;
        Event::dispatch(Notice::new("interface.stop", "stopped", &observed)).await?;
        let result = observed.lock().unwrap().clone();
        Response::json(StatusCode::OK, result)
    })
}

struct ClassListener(Record);

struct ListenerLabel(&'static str);

impl Listener for ClassListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Payload,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        self.0.handle(event, context)
    }
}

fn class_listeners(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let dispatcher = Event::dispatcher()?;
        let observed = Arc::new(Mutex::new(Vec::new()));
        assert!(dispatcher.listing().iter().any(|(name, labels)| {
            name == "class.ready" && labels == &[std::any::type_name::<ClassListener>()]
        }));
        Event::listen_class::<ClassListener>("class.facade", Delivery::Sync)?;
        for (name, note) in [
            ("class.ready", "first"),
            ("class.facade", "second"),
            ("class.stop", "stopped"),
        ] {
            Event::dispatch(Notice::new(name, note, &observed)).await?;
        }
        let db = Db::connection(None)?;
        let mut transaction = db.begin_transaction().await?;
        Event::dispatch_in_transaction(
            Notice::new("class.committed", "committed", &observed),
            &mut transaction,
        )
        .await?;
        assert_eq!(observed.lock().unwrap().len(), 3);
        transaction.commit().await?;
        let mut transaction = db.begin_transaction().await?;
        Event::dispatch_in_transaction(
            Notice::new("class.committed", "rolled back", &observed),
            &mut transaction,
        )
        .await?;
        transaction.roll_back(None).await?;
        let result = observed.lock().unwrap().clone();
        Response::json(StatusCode::OK, result)
    })
}

fn routing(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/events", run)?;
    router.get("/isolated", isolated)?;
    router.get("/deferred", deferred)?;
    router.get("/interfaces", interface_listeners)?;
    router.get("/class-listeners", class_listeners)?;
    Ok(router)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

async fn start(app: Application) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (address, server)
}

async fn request(address: SocketAddr, path: &str) -> Json {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }).await.unwrap()
}

async fn verify(connection: Json) {
    let mut app = Application::configure(std::env::temp_dir())
        .with_events(DiscoverEvents::new(&[register]))
        .with_routing(Routing::new(routing))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    config.set("database.default", json!("events")).unwrap();
    config
        .set("database.connections.events", connection)
        .unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    dispatcher.subscribe(&Extra).unwrap();
    assert!(dispatcher.has_listeners("notice.extra"));
    for sql in [
        "drop table if exists event_records",
        "create table event_records (id bigint primary key)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let other = Application::configure(std::env::temp_dir())
        .with_routing(Routing::new(routing))
        .create()
        .unwrap();
    let (address, server) = start(app).await;
    let (other_address, other_server) = start(other).await;
    let (result, other) = tokio::join!(
        request(address, "/events"),
        request(other_address, "/isolated")
    );
    assert_eq!(
        result,
        json!([
            "first:stop",
            "stop:stop",
            "answer:until",
            "now:outer",
            "wild:outer",
            "whole:kept",
            "wild:kept",
            "after:outer",
            "subscriber:registered"
        ])
    );
    assert_eq!(other, json!([]));
    assert_eq!(
        request(address, "/deferred").await,
        json!([
            "answer:immediate",
            "skipped:immediate",
            "wild:immediate",
            "whole:nested",
            "wild:nested",
            "whole:first",
            "wild:first",
            "whole:last",
            "wild:last"
        ])
    );

    let result = db
        .transaction(
            |transaction| {
                Box::pin(async move {
                    transaction
                        .insert("insert into event_records (id) values (2)", &[])
                        .await?;
                    transaction.after_commit(|| {
                        Box::pin(async { Err(DatabaseError::InvalidQuery("callback failure")) })
                    })?;
                    Ok(())
                })
            },
            3,
        )
        .await;
    assert!(matches!(result, Err(DatabaseError::AfterCommit { .. })));
    assert_eq!(
        db.scalar("select count(*) from event_records", &[])
            .await
            .unwrap(),
        Value::Integer(2)
    );
    dispatcher.forget("notice.*");
    assert!(!dispatcher.has_listeners("notice.extra"));
    server.abort();
    other_server.abort();
    db.statement("drop table event_records", &[]).await.unwrap();
}

#[tokio::test]
async fn event_contracts_run_after_wildcards_over_http() {
    fn register_interfaces(registry: &mut ListenerRegistry) -> EventResult<()> {
        for (event, label) in [
            ("interface.ready", "exact"),
            ("interface.*", "wild"),
            ("contracts.notice", "notice"),
            ("contracts.delivery", "delivery"),
            ("contracts.*", "skip-interface-wildcard"),
            ("interface.stop", "stop"),
        ] {
            registry.listen(event, Arc::new(Record(label)), Delivery::Sync)?;
        }
        Ok(())
    }
    let app = Application::configure(std::env::temp_dir())
        .with_events(DiscoverEvents::new(&[register_interfaces]))
        .with_routing(Routing::new(routing))
        .create()
        .unwrap();
    let (address, server) = start(app).await;
    assert_eq!(
        request(address, "/interfaces").await,
        json!([
            "exact:normal",
            "wild:normal",
            "notice:normal",
            "delivery:normal",
            "stop:stopped"
        ])
    );
    server.abort();
}

async fn verify_class_listeners(connection: Json) {
    fn register_classes(registry: &mut ListenerRegistry) -> EventResult<()> {
        registry.listen_class::<ClassListener>("class.ready", Delivery::Sync)?;
        registry.listen("class.stop", Arc::new(Record("stop")), Delivery::Sync)?;
        registry.listen_class::<ClassListener>("class.stop", Delivery::Sync)?;
        registry.listen_class::<ClassListener>("class.committed", Delivery::AfterCommit)
    }

    for label in ["first-app", "second-app"] {
        let mut app = Application::configure(std::env::temp_dir())
            .with_events(DiscoverEvents::new(&[register_classes]))
            .with_routing(Routing::new(routing))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let config = app.config().unwrap();
        config.set("database.default", json!("events")).unwrap();
        config
            .set("database.connections.events", connection.clone())
            .unwrap();

        let dispatcher = app.container().make::<Dispatcher>().unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        assert!(matches!(
            dispatcher
                .dispatch(
                    Arc::new(Notice::new("class.ready", "unbound", &observed)),
                    &mut EventContext::new(),
                )
                .await,
            Err(EventError::Unavailable(_))
        ));
        assert!(observed.lock().unwrap().is_empty());

        let resolved = Arc::new(AtomicUsize::new(0));
        let resolutions = resolved.clone();
        app.container()
            .instance(Arc::new(ListenerLabel(label)))
            .unwrap();
        app.container()
            .bind_async::<ClassListener, _>(move |container| {
                resolutions.fetch_add(1, Ordering::Relaxed);
                Box::pin(async move {
                    let label = container.make_async::<ListenerLabel>().await?;
                    Ok(Arc::new(ClassListener(Record(label.0))))
                })
            })
            .await
            .unwrap();
        let (address, server) = start(app).await;
        assert_eq!(resolved.load(Ordering::Relaxed), 0);
        assert_eq!(
            request(address, "/class-listeners").await,
            json!([
                format!("{label}:first"),
                format!("{label}:second"),
                "stop:stopped",
                format!("{label}:committed")
            ])
        );
        assert_eq!(resolved.load(Ordering::Relaxed), 4);
        server.abort();
    }
}

#[tokio::test]
async fn class_listeners_resolve_lazily_in_their_application() {
    verify_class_listeners(json!({"driver":"sqlite", "database":":memory:"})).await;
    if let Ok(url) = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL") {
        verify_class_listeners(json!({"url":url})).await;
    }
}

#[tokio::test]
async fn event_delivery_over_http_on_sqlite() {
    verify(json!({"driver":"sqlite", "database":":memory:"})).await;
}

#[tokio::test]
async fn event_delivery_over_http_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await;
    }
}

struct ProductionListener;

impl ShouldBeDiscovered for ProductionListener {
    fn should_be_discovered(app: &Application) -> EventResult<bool> {
        if app
            .environment(Some("broken".into()))
            .map(bool::from)
            .map_err(EventError::Unavailable)?
        {
            return Err(EventError::InvalidRegistration);
        }
        app.environment(Some("production".into()))
            .map(bool::from)
            .map_err(EventError::Unavailable)
    }
}

impl Listener for ProductionListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Payload,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        static RECORD: Record = Record("production");
        RECORD.handle(event, context)
    }
}

fn production_listener(registry: &mut ListenerRegistry) -> EventResult<()> {
    registry.listen("order.ready", Arc::new(ProductionListener), Delivery::Sync)
}

fn discovery_manifest() -> DiscoverEvents {
    DiscoverEvents::new(&[]).withListener::<ProductionListener>(production_listener)
}

#[tokio::test]
async fn discovery_policy_filters_http_and_artisan_per_application() {
    let production = Application::configure(std::env::temp_dir())
        .with_config(|_, _| Ok([("app".into(), json!({"env":"production"}))].into()))
        .with_events(discovery_manifest())
        .with_routing(Routing::new(routing))
        .create()
        .unwrap();
    let testing = Application::configure(std::env::temp_dir())
        .with_config(|_, _| Ok([("app".into(), json!({"env":"testing"}))].into()))
        .with_events(discovery_manifest())
        .with_routing(Routing::new(routing))
        .create()
        .unwrap();
    let ((production, production_server), (testing, testing_server)) =
        tokio::join!(start(production), start(testing));
    let (included, excluded) = tokio::join!(
        request(production, "/isolated"),
        request(testing, "/isolated"),
    );
    assert_eq!(included, json!(["production:isolated"]));
    assert_eq!(excluded, json!([]));
    production_server.abort();
    testing_server.abort();

    let mut production = TestApp::create(|builder| {
        builder
            .with_config(|_, _| Ok([("app".into(), json!({"env":"production"}))].into()))
            .with_events(discovery_manifest())
    })
    .unwrap();
    let mut testing = TestApp::create(|builder| builder.with_events(discovery_manifest())).unwrap();
    let arguments = ["event:list", "--event=order.ready", "--json"];
    let (included, excluded) =
        tokio::join!(production.artisan(arguments), testing.artisan(arguments));
    included.assert_successful();
    excluded.assert_successful();
    let entries: Json = serde_json::from_str(&included.stdout).unwrap();
    assert_eq!(entries[0]["event"], "order.ready");
    assert_eq!(
        entries[0]["listeners"],
        json!([std::any::type_name::<ProductionListener>()])
    );
    assert_eq!(
        serde_json::from_str::<Json>(&excluded.stdout).unwrap(),
        json!([])
    );
    testing
        .app()
        .container()
        .make::<Dispatcher>()
        .unwrap()
        .register(production_listener)
        .unwrap();
    assert!(
        testing
            .app()
            .container()
            .make::<Dispatcher>()
            .unwrap()
            .has_listeners("order.ready")
    );
}

#[tokio::test]
async fn failed_discovery_leaves_the_manifest_unregistered() {
    struct FailingPolicy;

    impl Listener for FailingPolicy {
        fn handle<'a>(
            &'a self,
            _: &'a dyn Payload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            unreachable!("the policy rejects discovery")
        }
    }

    impl ShouldBeDiscovered for FailingPolicy {
        fn should_be_discovered(_: &Application) -> EventResult<bool> {
            Err(EventError::InvalidRegistration)
        }
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_events(DiscoverEvents::new(&[register]).withListener::<FailingPolicy>(register))
    })
    .unwrap();
    assert!(app.boot().await.is_err());
    let dispatcher = app.app().container().make::<Dispatcher>().unwrap();
    assert!(!dispatcher.has_listeners("order.ready"));
}

#[tokio::test]
#[ignore = "Spawned by the event cache acceptance test"]
async fn event_cache_artisan_fixture() {
    let root = std::env::var_os("EVENT_CACHE_TEST_ROOT").unwrap();
    let app = Application::configure(root.into())
        .with_config(|_, _| {
            let environment = std::env::var("EVENT_CACHE_TEST_ENV").unwrap();
            Ok([("app".into(), json!({"env": environment}))].into())
        })
        .with_events(discovery_manifest())
        .create()
        .unwrap();
    let arguments = std::env::var("EVENT_CACHE_TEST_COMMAND").unwrap();
    let arguments: Vec<_> = arguments.split_ascii_whitespace().map(Into::into).collect();
    assert_eq!(
        app.handle_command(&arguments).await.unwrap(),
        std::env::var("EVENT_CACHE_TEST_EXIT")
            .unwrap()
            .parse::<i32>()
            .unwrap()
    );
}

#[tokio::test]
async fn event_cache_survives_process_restart_and_clears_per_application() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let run = |root: &std::path::Path, environment: &str, command: &str, exit: &str| {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "event_cache_artisan_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("EVENT_CACHE_TEST_ROOT", root)
            .env("EVENT_CACHE_TEST_ENV", environment)
            .env("EVENT_CACHE_TEST_COMMAND", command)
            .env("EVENT_CACHE_TEST_EXIT", exit)
            .env_remove("APP_EVENTS_CACHE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };

    let list = "event:list --event=order.ready --json";
    let cache_path = root.path().join("storage/framework/cache/events.json");
    assert!(
        run(root.path(), "production", "event:cache", "0").contains("Events cached successfully.")
    );
    let cached = std::fs::read(&cache_path).unwrap();
    assert!(run(root.path(), "testing", list, "0").contains("ProductionListener"));
    assert!(run(other.path(), "testing", list, "0").contains("[]"));
    let application = || {
        Application::configure(root.path().to_owned())
            .with_config(|_, _| Ok([("app".into(), json!({"env":"testing"}))].into()))
            .with_events(discovery_manifest())
            .with_routing(Routing::new(routing))
            .create()
            .unwrap()
    };
    let (address, server) = start(application()).await;
    assert_eq!(
        request(address, "/isolated").await,
        json!(["production:isolated"])
    );
    server.abort();

    run(root.path(), "broken", "event:cache", "1");
    assert_eq!(std::fs::read(&cache_path).unwrap(), cached);
    assert!(
        run(root.path(), "testing", "event:clear", "0")
            .contains("Cached events cleared successfully.")
    );
    assert!(!cache_path.exists());
    assert!(run(root.path(), "testing", list, "0").contains("[]"));
    run(root.path(), "testing", "event:cache", "0");
    assert!(run(root.path(), "production", list, "0").contains("[]"));
    std::fs::write(
        &cache_path,
        br#"{"version":1,"decisions":[["old-listener",false]]}"#,
    )
    .unwrap();
    assert!(run(root.path(), "production", list, "0").contains("ProductionListener"));
    std::thread::scope(|scope| {
        scope.spawn(|| run(root.path(), "production", "event:cache", "0"));
        scope.spawn(|| run(root.path(), "testing", "event:cache", "0"));
    });
    let snapshot: Json = serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    assert_eq!(snapshot["decisions"].as_array().unwrap().len(), 1);
    std::fs::write(&cache_path, b"{corrupt").unwrap();
    assert!(run(root.path(), "production", list, "0").contains("ProductionListener"));
    run(root.path(), "testing", "event:clear", "0");
    run(root.path(), "testing", "event:clear", "0");

    std::fs::write(
        root.path().join(".env"),
        "APP_EVENTS_CACHE=private/discovery.json\n",
    )
    .unwrap();
    let custom = root.path().join("private/discovery.json");
    run(root.path(), "production", "event:cache", "0");
    assert!(custom.is_file() && !cache_path.exists());
    assert!(run(root.path(), "testing", list, "0").contains("ProductionListener"));
    let (address, server) = start(application()).await;
    assert_eq!(
        request(address, "/isolated").await,
        json!(["production:isolated"])
    );
    server.abort();
    run(root.path(), "testing", "event:clear", "0");
    assert!(!custom.exists());
    assert!(run(root.path(), "testing", list, "0").contains("[]"));
}

#[test]
fn test_app_preserves_inherited_event_cache_file() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("deployment.json");
    std::fs::write(&cache, b"unchanged deployment cache").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "test_app_event_cache_fixture", "--ignored"])
        .env("APP_EVENTS_CACHE", &cache)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(cache).unwrap(), b"unchanged deployment cache");
}

#[tokio::test]
#[ignore = "Spawned with a separate deployment environment"]
async fn test_app_event_cache_fixture() {
    let mut fixture = TestApp::create(|builder| builder).unwrap();
    std::fs::write(
        fixture.root().join(".env.testing"),
        "APP_EVENTS_CACHE=private/fixture.json\n",
    )
    .unwrap();
    let path = fixture
        .root()
        .canonicalize()
        .unwrap()
        .join("private/fixture.json");
    fixture.artisan(["event:cache"]).await.assert_successful();
    assert_eq!(fixture.app().get_cached_events_path(), Some(path.as_path()));
    assert!(path.is_file());
    fixture.artisan(["event:clear"]).await.assert_successful();
    assert!(!path.exists());
}
