use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _,
        events::{
            Dispatcher as _, Event as Payload, Listener, ShouldDispatchAfterCommit, Subscriber,
        },
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{DB, DatabaseError, DatabaseManager, Value},
    events::{
        Delivery, DispatchResult, Dispatcher, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse,
    },
    foundation::{Application, configuration::Routing, events::DiscoverEvents, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::Event,
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
                let connection = DB::connection(None).map_err(EventError::Transaction)?;
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

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
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

        let db = DB::connection(None).map_err(internal)?;
        let mut transaction = db.beginTransaction().await.map_err(internal)?;
        transaction
            .insert("insert into event_records (id) values (1)", &[])
            .await
            .map_err(internal)?;
        Event::dispatchInTransaction(
            Notice::new("order.completed", "outer", &observed),
            &mut transaction,
        )
        .await?;
        transaction.beginTransaction().await.map_err(internal)?;
        let result = Event::dispatchAfterCommit(
            Notice::new("order.whole", "kept", &observed),
            &mut transaction,
        )
        .await?;
        assert!(matches!(result, DispatchResult::Deferred));
        transaction.commit().await.map_err(internal)?;

        transaction.beginTransaction().await.map_err(internal)?;
        transaction.beginTransaction().await.map_err(internal)?;
        Event::dispatchAfterCommit(
            Notice::new("order.whole", "discarded", &observed),
            &mut transaction,
        )
        .await?;
        transaction.commit().await.map_err(internal)?;
        transaction.rollBack(Some(1)).await.map_err(internal)?;
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
        transaction.commit().await.map_err(internal)?;

        let mut rolled_back = db.beginTransaction().await.map_err(internal)?;
        rolled_back
            .insert("insert into event_records (id) values (3)", &[])
            .await
            .map_err(internal)?;
        Event::dispatchAfterCommit(
            Notice::new("order.whole", "rolled back", &observed),
            &mut rolled_back,
        )
        .await?;
        rolled_back.rollBack(None).await.map_err(internal)?;
        assert_eq!(
            db.scalar("select count(*) from event_records", &[])
                .await
                .map_err(internal)?,
            Value::Integer(1),
        );

        let mut discarded = db.beginTransaction().await.map_err(internal)?;
        Event::dispatchAfterCommit(
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

fn routing(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/events", run)?;
    router.get("/isolated", isolated)?;
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
        .withEvents(DiscoverEvents::new(&[register]))
        .withRouting(Routing::new(routing))
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
    assert!(dispatcher.hasListeners("notice.extra"));
    for sql in [
        "drop table if exists event_records",
        "create table event_records (id bigint primary key)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    let other = Application::configure(std::env::temp_dir())
        .withRouting(Routing::new(routing))
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

    let result = db
        .transaction(
            |transaction| {
                Box::pin(async move {
                    transaction
                        .insert("insert into event_records (id) values (2)", &[])
                        .await?;
                    transaction.afterCommit(|| {
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
    assert!(!dispatcher.hasListeners("notice.extra"));
    server.abort();
    other_server.abort();
    db.statement("drop table event_records", &[]).await.unwrap();
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
