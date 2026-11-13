use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _,
        foundation::Application as _,
        http::Kernel as _,
        queue::{Job, JobHandler, Queue as QueueContract},
    },
    database::{Connection, DatabaseManager},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{
        Delay, DispatchOptions, DispatchResult, JobContext, Payload, QueueError, QueueFuture,
        QueueManager, QueueResult,
    },
    routing::{Router, RoutingError},
    support::facades::Queue,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

const JOB: &str = "emails.send";

#[derive(Default)]
struct Handler(Mutex<Vec<(String, u32, u64)>>);

impl JobHandler for Handler {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let value = payload.data()["user_id"]
                .as_u64()
                .ok_or(QueueError::InvalidPayload)?;
            assert_eq!(payload.version(), 1);
            if let Some(transaction) = context.transaction() {
                assert_eq!(transaction.transactionLevel(), 1);
                transaction
                    .scalar("select 1", &[])
                    .await
                    .map_err(|source| QueueError::Execution {
                        source: Box::new(source),
                    })?;
            }
            self.0
                .lock()
                .unwrap()
                .push((context.queue().into(), context.attempts(), value));
            Ok(())
        })
    }
}

fn payload() -> Payload {
    Payload::new(JOB, json!({"user_id":u64::MAX})).unwrap()
}

fn send(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let connection = request.route("driver").map(str::to_owned);
        let result = Queue::dispatch(
            payload(),
            DispatchOptions {
                connection,
                ..Default::default()
            },
            None,
        )
        .await?;
        Response::json(
            StatusCode::OK,
            json!(match result {
                DispatchResult::Completed => "sync".to_owned(),
                DispatchResult::Queued(id) => id,
                DispatchResult::Deferred => unreachable!(),
            }),
        )
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/queue/{driver}", send)?;
    Ok(router)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
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

async fn request(address: SocketAddr, driver: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("GET /queue/{driver} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }).await.unwrap()
}

async fn application(
    db: Value,
    redis_port: Option<u16>,
    table: &str,
) -> (Application, Arc<QueueManager>, Connection, Arc<Handler>) {
    let mut app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::new(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    config.set("database.default", json!("queue-test")).unwrap();
    config.set("database.connections.queue-test", db).unwrap();
    config.set("queue.default", json!("database")).unwrap();
    config
        .set(
            "queue.connections",
            json!({
                "database":{"driver":"database","table":table,"retry_after":1},
                "after":{"driver":"database","table":table,"retry_after":1,"after_commit":true},
                "redis":{"driver":"redis","retry_after":1},
                "blocking":{"driver":"redis","retry_after":1,"block_for":2},
                "sync":{"driver":"sync"},
            }),
        )
        .unwrap();
    if let Some(port) = redis_port {
        config
            .set(
                "database.redis",
                json!({
                    "options":{"prefix":format!("openrust-test:{table}:")},
                    "default":{"url":format!("redis://127.0.0.1:{port}/")}
                }),
            )
            .unwrap();
    }
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let queue = app.container().make::<QueueManager>().unwrap();
    let handler = Arc::new(Handler::default());
    queue.register(JOB, handler.clone()).unwrap();
    (app, queue, db, handler)
}

struct ExtensionQueue {
    name: String,
    inner: Arc<dyn QueueContract>,
}

impl QueueContract for ExtensionQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        self.inner.size(queue)
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        self.inner.push(payload, queue)
    }

    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        self.inner.later(delay, payload, queue)
    }

    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        self.inner.pop(queue)
    }
}

#[tokio::test]
async fn custom_queue_connections_resolve_per_application_and_dispatch_over_http() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("queue.sqlite");
    let config = json!({"driver":"sqlite","database":database});
    let (app, manager, _, handler) = application(config.clone(), None, "jobs").await;
    let (other, other_manager, _, _) = application(config, None, "jobs").await;
    for app in [&app, &other] {
        app.config()
            .unwrap()
            .set(
                "queue.connections.extension",
                json!({"driver":"custom","delegate":"sync"}),
            )
            .unwrap();
    }
    for manager in [&manager, &other_manager] {
        assert!(matches!(
            manager.connection(Some("extension")),
            Err(QueueError::UnsupportedDriver(driver)) if driver == "custom"
        ));
    }
    manager.extend("custom", |container, name, settings| {
        let manager = container
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)?;
        let inner = manager.connection(settings["delegate"].as_str())?;
        Ok(Arc::new(ExtensionQueue {
            name: name.into(),
            inner,
        }))
    });
    let first = manager.connection(Some("extension")).unwrap();
    manager.extend("custom", |_, _, _| Err(QueueError::InvalidConfiguration));
    assert!(Arc::ptr_eq(
        &first,
        &manager.connection(Some("extension")).unwrap()
    ));
    assert!(matches!(
        other_manager.connection(Some("extension")),
        Err(QueueError::UnsupportedDriver(_))
    ));
    let (address, server) = start(app).await;
    assert_eq!(request(address, "extension").await, "sync");
    assert_eq!(
        handler.0.lock().unwrap().as_slice(),
        &[("default".into(), 1, u64::MAX)]
    );
    server.abort();
}

async fn exercise_storage(
    manager: &QueueManager,
    connection: &str,
    first_id: &str,
    handler: &Handler,
) {
    let queue = manager.connection(Some(connection)).unwrap();
    assert_eq!(queue.size(None).await.unwrap(), 1);
    let (one, two) = tokio::join!(queue.pop(None), queue.pop(None));
    let (one, two) = (one.unwrap(), two.unwrap());
    assert_ne!(one.is_some(), two.is_some());
    let mut first = one.or(two).unwrap();
    assert_eq!(first.get_job_id(), first_id);
    assert_eq!(first.attempts(), 1);
    let uuid = first.uuid().to_owned();
    // The expired handle may not acknowledge a later reservation of the same job.
    sleep(Duration::from_millis(1100)).await;
    let mut second = queue.pop(None).await.unwrap().unwrap();
    assert_eq!(second.uuid(), uuid);
    assert_eq!(second.attempts(), 2);
    assert!(matches!(
        first.delete().await,
        Err(QueueError::LostReservation)
    ));
    second.release(Duration::ZERO).await.unwrap();
    assert!(second.is_released());
    let mut third = queue.pop(None).await.unwrap().unwrap();
    assert_eq!(third.uuid(), uuid);
    assert_eq!(third.attempts(), 3);
    let mut context = JobContext::default();
    third.fire(&mut context).await.unwrap();
    third.delete().await.unwrap();
    assert!(third.is_deleted());
    assert_eq!(queue.size(None).await.unwrap(), 0);
    assert_eq!(
        handler.0.lock().unwrap().as_slice(),
        &[
            ("default".into(), 1, u64::MAX),
            ("default".into(), 3, u64::MAX)
        ]
    );

    queue
        .later(
            Delay::Until(std::time::SystemTime::now() + Duration::from_secs(2)),
            &payload(),
            Some("delayed"),
        )
        .await
        .unwrap();
    assert_eq!(queue.size(Some("delayed")).await.unwrap(), 1);
    assert!(queue.pop(Some("delayed")).await.unwrap().is_none());
    sleep(Duration::from_millis(3100)).await;
    let mut delayed = queue.pop(Some("delayed")).await.unwrap().unwrap();
    assert_eq!(delayed.get_queue(), "delayed");
    delayed.delete().await.unwrap();
}

async fn verify_database(db_config: Value) {
    let table = format!("queue_test_{:016x}", fastrand::u64(..));
    let (app, manager, db, handler) = application(db_config, None, &table).await;
    db.getSchemaBuilder()
        .create(&table, |table| {
            table.id();
            table.string("queue", 255).index();
            table.longText("payload");
            table.unsignedInteger("attempts");
            table.unsignedBigInteger("reserved_at").nullable();
            table.unsignedBigInteger("available_at");
            table.unsignedBigInteger("created_at");
        })
        .await
        .unwrap();
    let (address, server) = start(app).await;
    assert_eq!(request(address, "sync").await, "sync");
    let id = request(address, "database").await;
    exercise_storage(&manager, "database", &id, &handler).await;
    let queue = manager.connection(None).unwrap();
    let mut transaction = db.beginTransaction().await.unwrap();
    let inserted = manager
        .dispatch(
            payload(),
            DispatchOptions::default(),
            Some(&mut transaction),
        )
        .await
        .unwrap();
    assert!(matches!(inserted, DispatchResult::Queued(_)));
    assert_eq!(transaction.table(&table).count("*").await.unwrap(), 1);
    transaction.rollBack(None).await.unwrap();
    assert_eq!(queue.size(None).await.unwrap(), 0);

    let deferred = || DispatchOptions {
        connection: Some("after".into()),
        ..Default::default()
    };
    let mut transaction = db.beginTransaction().await.unwrap();
    assert_eq!(
        manager
            .dispatch(payload(), deferred(), Some(&mut transaction))
            .await
            .unwrap(),
        DispatchResult::Deferred
    );
    transaction.beginTransaction().await.unwrap();
    manager
        .dispatch(payload(), deferred(), Some(&mut transaction))
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    assert_eq!(transaction.table(&table).count("*").await.unwrap(), 0);
    transaction.commit().await.unwrap();
    assert_eq!(queue.size(None).await.unwrap(), 1);
    queue
        .pop(None)
        .await
        .unwrap()
        .unwrap()
        .delete()
        .await
        .unwrap();

    let mut transaction = db.beginTransaction().await.unwrap();
    manager
        .dispatch(
            payload(),
            DispatchOptions {
                connection: Some("sync".into()),
                after_commit: Some(true),
                ..Default::default()
            },
            Some(&mut transaction),
        )
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    assert_eq!(handler.0.lock().unwrap().len(), 2);
    let mut transaction = db.beginTransaction().await.unwrap();
    let result = manager
        .dispatch(
            payload(),
            DispatchOptions {
                connection: Some("sync".into()),
                ..Default::default()
            },
            Some(&mut transaction),
        )
        .await
        .unwrap();
    assert_eq!(result, DispatchResult::Completed);
    transaction.rollBack(None).await.unwrap();
    assert_eq!(handler.0.lock().unwrap().len(), 3);
    let unknown = manager
        .dispatch(
            Payload::new("missing", ()).unwrap(),
            DispatchOptions {
                connection: Some("sync".into()),
                ..Default::default()
            },
            None,
        )
        .await;
    assert!(matches!(unknown, Err(QueueError::UnknownJob(_))));
    server.abort();
    db.getSchemaBuilder().dropIfExists(&table).await.unwrap();
}

#[tokio::test]
async fn database_queue_over_http_on_sqlite() {
    verify_database(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn database_queue_over_http_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify_database(json!({"url":url})).await;
    }
}

#[tokio::test]
async fn redis_queue_over_http_and_blocking_reservation() {
    let port = std::env::var("OPENRUST_TEST_REDIS_PORT").ok();
    assert!(port.is_some() || std::env::var_os("OPENRUST_REQUIRE_QUEUE_REDIS").is_none());
    let Some(port) = port else { return };
    let table = format!("redis_test_{:016x}", fastrand::u64(..));
    let (app, manager, db, handler) = application(
        json!({"driver":"sqlite","database":":memory:"}),
        Some(port.parse().unwrap()),
        &table,
    )
    .await;
    let (address, server) = start(app).await;
    assert_eq!(request(address, "sync").await, "sync");
    let id = request(address, "redis").await;
    exercise_storage(&manager, "redis", &id, &handler).await;

    let queue = manager.connection(Some("redis")).unwrap();
    let blocking = manager.connection(Some("blocking")).unwrap();
    let reserved = tokio::spawn(async move { blocking.pop(Some("notify")).await });
    sleep(Duration::from_millis(100)).await;
    queue.push(&payload(), Some("notify")).await.unwrap();
    let mut job = timeout(Duration::from_secs(3), reserved)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    job.delete().await.unwrap();

    let mut transaction = db.beginTransaction().await.unwrap();
    let options = DispatchOptions {
        connection: Some("redis".into()),
        after_commit: Some(true),
        ..Default::default()
    };
    assert_eq!(
        manager
            .dispatch(payload(), options, Some(&mut transaction))
            .await
            .unwrap(),
        DispatchResult::Deferred
    );
    assert_eq!(queue.size(None).await.unwrap(), 0);
    transaction.commit().await.unwrap();
    assert_eq!(queue.size(None).await.unwrap(), 1);
    queue
        .pop(None)
        .await
        .unwrap()
        .unwrap()
        .delete()
        .await
        .unwrap();
    server.abort();
}
