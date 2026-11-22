#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use std::{
    ffi::OsString,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{AuthIdentifier, AuthManager},
    broadcasting::{
        BroadcastChannel, BroadcastException, BroadcastFuture, BroadcastManager, BroadcastPayload,
        BroadcastResult, Channel, ChannelAuthorization, ChannelBindings, ChannelModels,
        ChannelOptions, ChannelParameters, PresenceChannel, PrivateChannel,
    },
    contracts::{
        auth::Authenticatable,
        broadcasting::{
            Broadcaster, ChannelHandler, Factory, ShouldBeUnique, ShouldBroadcast,
            ShouldBroadcastNow,
        },
        events::{Dispatcher as _, Event, ShouldDispatchAfterCommit},
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{
        DatabaseManager, Value as DbValue,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    events::{DispatchResult, Dispatcher, EventContext},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{JobContext, QueueManager, events::UniqueJobSkipped},
    routing::{Router, RoutingError},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Broadcast, DB},
    },
};
use serde_json::{Value, json};

type Delivery = (Vec<String>, String, BroadcastPayload, Option<String>);

#[derive(Default)]
struct Recorder {
    delivered: Mutex<Vec<Delivery>>,
    fail_once: AtomicBool,
}

impl Broadcaster for Recorder {
    fn auth<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>> {
        Box::pin(async move {
            let channel = request
                .query(Some("channel_name"))
                .ok()
                .flatten()
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or(BroadcastException::InvalidChannel)?;
            let (channel, presence) = if let Some(name) = channel.strip_prefix("private-") {
                (name, false)
            } else if let Some(name) = channel.strip_prefix("presence-") {
                (name, true)
            } else {
                return Err(BroadcastException::AccessDenied);
            };
            let registry = Broadcast::channelRegistry()?;
            let result = registry.authorizeRequest(channel, request).await?;
            match (&result, presence) {
                (ChannelAuthorization::Allowed, false)
                | (ChannelAuthorization::Presence(_), true) => {
                    self.validAuthenticationResponse(request, result).await
                }
                _ => Err(BroadcastException::AccessDenied),
            }
        })
    }

    fn validAuthenticationResponse<'a>(
        &'a self,
        _: &'a mut Request,
        result: ChannelAuthorization,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>> {
        Box::pin(async move {
            let response = match result {
                ChannelAuthorization::Presence(data) => json!({"member": data}),
                ChannelAuthorization::Allowed => json!({"authorized": true}),
                ChannelAuthorization::Denied | ChannelAuthorization::Abstain => {
                    return Err(BroadcastException::AccessDenied);
                }
            };
            Response::json(StatusCode::OK, response)
                .map_err(|source| BroadcastException::Backend(Box::new(source)))
        })
    }

    fn broadcast<'a>(
        &'a self,
        channels: &'a [BroadcastChannel],
        event: &'a str,
        payload: &'a BroadcastPayload,
        socket: Option<&'a str>,
    ) -> BroadcastFuture<'a, BroadcastResult<()>> {
        Box::pin(async move {
            if self.fail_once.swap(false, Ordering::SeqCst) {
                return Err(BroadcastException::Backend(
                    std::io::Error::other("retry").into(),
                ));
            }
            self.delivered.lock().unwrap().push((
                channels
                    .iter()
                    .map(|channel| channel.wire_name().into_owned())
                    .collect(),
                event.to_owned(),
                payload.clone(),
                socket.map(str::to_owned),
            ));
            Ok(())
        })
    }
}

struct Identity;
impl Authenticatable for Identity {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        AuthIdentifier::String("42".into())
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        None
    }
    fn getRememberToken(&self) -> Option<&str> {
        None
    }
    fn setRememberToken(&mut self, _: Option<String>) {}
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
}

struct Register(Arc<Recorder>);
impl ServiceProvider for Register {
    fn boot<'a>(&'a mut self, app: &'a Application) -> BroadcastFuture<'a, ProviderResult> {
        Box::pin(async move {
            let recorder = self.0.clone();
            app.make::<BroadcastManager>()
                .await?
                .extend("record", move |_| Ok(recorder.clone()))?;
            let auth = app.make::<AuthManager>().await?;
            auth.viaRequest("header", |request| {
                Box::pin(async move {
                    Ok(request
                        .headers()
                        .get("x-identity")
                        .is_some_and(|value| value == "42")
                        .then(|| Box::new(Identity) as Box<dyn Authenticatable>))
                })
            });
            auth.viaRequest("empty", |_| Box::pin(async { Ok(None) }));
            let dispatcher = app.make::<Dispatcher>().await?;
            dispatcher.registerBroadcastNow::<Notice<1>>();
            dispatcher.markAfterCommit::<Notice<1>>();
            dispatcher.registerBroadcast::<Notice<2>>();
            dispatcher.registerUniqueBroadcast::<UniqueNotice<1>>();
            dispatcher.registerUniqueBroadcast::<UniqueNotice<2>>();
            dispatcher.registerUniqueBroadcast::<UniqueNotice<3>>();
            Ok(())
        })
    }
}

struct Notice<const MODE: u8>(bool);
impl<const MODE: u8> Event for Notice<MODE> {
    fn name(&self) -> &str {
        "notice"
    }
}
impl<const MODE: u8> ShouldBroadcast for Notice<MODE> {
    fn broadcastOn(&self) -> Vec<BroadcastChannel> {
        vec![
            BroadcastChannel::Public(Channel {
                name: "news".into(),
            }),
            BroadcastChannel::Private(PrivateChannel {
                name: "orders.42".into(),
            }),
            BroadcastChannel::Presence(PresenceChannel {
                name: "rooms.42".into(),
            }),
        ]
    }
    fn broadcastAs(&self) -> Option<&str> {
        (MODE == 2).then_some("notice.queued")
    }
    fn broadcastData(&self) -> BroadcastPayload {
        BroadcastPayload::from_iter([("id".into(), json!(42))])
    }
    fn broadcastWhen(&self) -> bool {
        self.0
    }
    fn broadcastQueue(&self) -> Option<&str> {
        Some("broadcasts")
    }
    fn broadcastAfterCommit(&self) -> Option<bool> {
        Some(true)
    }
}
impl ShouldBroadcastNow for Notice<1> {}
impl ShouldDispatchAfterCommit for Notice<1> {}

struct UniqueNotice<const KIND: u8>(String);

impl<const KIND: u8> Event for UniqueNotice<KIND> {
    fn name(&self) -> &str {
        "unique.notice"
    }
}

impl<const KIND: u8> ShouldBroadcast for UniqueNotice<KIND> {
    fn broadcastOn(&self) -> Vec<BroadcastChannel> {
        vec!["unique".into()]
    }
    fn broadcastData(&self) -> BroadcastPayload {
        BroadcastPayload::from_iter([("id".into(), json!(self.0)), ("kind".into(), json!(KIND))])
    }
    fn broadcastQueue(&self) -> Option<&str> {
        Some("broadcasts")
    }
    fn shouldBroadcastNow(&self) -> bool {
        KIND == 3
    }
    fn broadcastAfterCommit(&self) -> Option<bool> {
        Some(true)
    }
}

impl<const KIND: u8> ShouldBeUnique for UniqueNotice<KIND> {
    fn uniqueId(&self) -> &str {
        &self.0
    }
    fn uniqueFor(&self) -> Option<u64> {
        Some(if self.0 == "expiring" { 1 } else { 60 })
    }
    fn uniqueVia(&self) -> Option<&str> {
        Some("event-locks")
    }
}

struct Room(ModelState);

impl Model for Room {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("channel_rooms").timestamps(None, None);
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
    fn getRouteKeyName() -> &'static str {
        "slug"
    }
}

struct RoomChannel;

impl ChannelHandler for RoomChannel {
    fn joinBound<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        parameters: &'a ChannelParameters,
        models: &'a ChannelModels,
    ) -> BroadcastFuture<'a, BroadcastResult<ChannelAuthorization>> {
        Box::pin(async move {
            if let Some(value) = models.get::<String>("explicit") {
                return Ok(if value == "bound" {
                    ChannelAuthorization::Allowed
                } else {
                    ChannelAuthorization::Denied
                });
            }
            let room = models
                .get::<Room>("room")
                .expect("room argument should be bound");
            let owner = room.getAttributeValue("owner").unwrap();
            let allowed = match owner {
                Some(DbValue::Text(owner)) => {
                    user.getAuthIdentifier() == AuthIdentifier::String(owner)
                }
                _ => false,
            };
            Ok(if allowed {
                ChannelAuthorization::Presence(BroadcastPayload::from_iter([(
                    "room".into(),
                    json!(parameters["room"].clone()),
                )]))
            } else {
                ChannelAuthorization::Denied
            })
        })
    }
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.bind("explicit", |context| {
        Box::pin(async move { Ok((context.value() == "key").then(|| "bound".to_owned())) })
    });
    router.get("/broadcast/anonymous-now", anonymous_now)?;
    router.get("/broadcast/anonymous", anonymous_queued)?;
    router.get("/broadcast/unique/{id}", send_unique::<1>)?;
    router.get("/broadcast/unique-other/{id}", send_unique::<2>)?;
    router.get("/broadcast/unique-now/{id}", send_unique::<3>)?;
    router.get("/broadcast/commit/{id}", transaction_unique::<true>)?;
    router.get("/broadcast/rollback/{id}", transaction_unique::<false>)?;
    Ok(router)
}

fn anonymous_now(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Broadcast::on([
            BroadcastChannel::from("news"),
            PrivateChannel {
                name: "alerts".into(),
            }
            .into(),
        ])
        .r#as("anonymous.ready")
        .with(BroadcastPayload::from_iter([("ok".into(), json!(true))]))
        .toOthers(request)
        .sendNow()
        .await?;
        Broadcast::private("discarded")
            .via("null")
            .sendNow()
            .await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn anonymous_queued(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Broadcast::presence("lobby").send().await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn send_unique<const KIND: u8>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = request.route("id").unwrap().to_owned();
        openrust::support::facades::Event::dispatch(UniqueNotice::<KIND>(id)).await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn transaction_unique<const COMMIT: bool>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = request.route("id").unwrap().to_owned();
        let database = DB::connection(None)?;
        let mut transaction = database.beginTransaction().await?;
        openrust::support::facades::Event::dispatchInTransaction(
            UniqueNotice::<1>(id),
            &mut transaction,
        )
        .await?;
        if COMMIT {
            transaction.commit().await?;
        } else {
            transaction.rollBack(None).await?;
        }
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn channels() -> BroadcastResult<()> {
    let options = || ChannelOptions {
        guards: vec!["header".into()],
    };
    Broadcast::channelWithOptions(
        "orders.{id}",
        |user: &dyn Authenticatable, parameters: &ChannelParameters| {
            Ok(
                if user.getAuthIdentifier() == AuthIdentifier::String(parameters["id"].clone()) {
                    ChannelAuthorization::Allowed
                } else {
                    ChannelAuthorization::Denied
                },
            )
        },
        options(),
    )?;
    Broadcast::channelWithOptions(
        "rooms.{id}",
        |_: &dyn Authenticatable, parameters: &ChannelParameters| {
            Ok(ChannelAuthorization::Presence(BroadcastPayload::from_iter(
                [("room".into(), json!(parameters["id"]))],
            )))
        },
        options(),
    )?;
    Broadcast::channelWithBindings(
        "models.{room}",
        RoomChannel,
        ChannelOptions {
            guards: vec!["empty".into(), "header".into()],
        },
        ChannelBindings::default().model::<Room>("room"),
    )?;
    Broadcast::channelWithBindings(
        "explicit.{explicit}",
        RoomChannel,
        options(),
        ChannelBindings::default().model::<Room>("explicit"),
    )?;
    Broadcast::channelWithOptions(
        "overlap.{room}",
        |_: &dyn Authenticatable, parameters: &ChannelParameters| {
            Ok(if parameters["room"] == "deny" {
                ChannelAuthorization::Denied
            } else {
                ChannelAuthorization::Abstain
            })
        },
        options(),
    )?;
    for pattern in ["overlap.allow", "overlap.deny"] {
        Broadcast::channelWithOptions(
            pattern,
            |_: &dyn Authenticatable, _: &ChannelParameters| Ok(ChannelAuthorization::Allowed),
            options(),
        )?;
    }
    Ok(())
}

fn application(recorder: Arc<Recorder>) -> Application {
    application_at(recorder, &std::env::temp_dir())
}

fn application_at(recorder: Arc<Recorder>, root: &Path) -> Application {
    Application::configure(root.to_path_buf())
        .withConfig(|root, _| {
            let sqlite_file = root.join("broadcast-probe.sqlite");
            let database = if sqlite_file.is_file() { sqlite_file.to_string_lossy() } else { ":memory:".into() };
            Ok([
                ("app".into(), json!({"key": "x".repeat(32)})),
                ("auth".into(), json!({"guards":{"header":{"driver":"header"},"empty":{"driver":"empty"}}})),
                ("database".into(), json!({"default":"broadcast","connections":{"broadcast":{"driver":"sqlite","database":database}}})),
                ("session".into(), json!({"driver":"array","lottery":[0,100]})),
                ("broadcasting".into(), json!({"default":"probe","connections":{"probe":{"driver":"record"}}})),
                ("cache".into(), json!({"default":"array","stores":{"array":{"driver":"array"},"event-locks":{"driver":"file","path":"broadcast-locks"}}})),
                ("queue".into(), json!({"default":"db","connections":{"db":{"driver":"database","queue":"default","table":"jobs","retry_after":30}}})),
            ].into())
        })
        .withRouting(Routing::new(routes))
        .withBroadcasting(channels)
        .withProviders(vec![Box::new(Register(recorder))])
        .create().unwrap()
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

#[tokio::test]
async fn actual_channel_requests_use_guard_rules_and_presence_payloads() {
    let mut app = application(Arc::default());
    app.boot().await.unwrap();
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    db.getSchemaBuilder()
        .create("channel_rooms", |table| {
            table.id();
            table.string("slug", 100).unique();
            table.string("owner", 100);
        })
        .await
        .unwrap();
    db.statement(
        "insert into channel_rooms (slug, owner) values ('lobby', '42'), ('other', '99')",
        &[],
    )
    .await
    .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    for (method, channel, identity, status, expected) in [
        (
            "GET",
            "private-orders.42",
            "42",
            200,
            json!({"authorized": true}),
        ),
        (
            "POST",
            "presence-rooms.42",
            "42",
            200,
            json!({"member":{"room":"42"}}),
        ),
        ("GET", "private-orders.43", "42", 403, Value::Null),
        ("POST", "private-orders.42", "anonymous", 403, Value::Null),
        ("GET", "orders.42", "42", 403, Value::Null),
        ("GET", "private-missing.42", "42", 403, Value::Null),
        ("PUT", "private-orders.42", "42", 405, Value::Null),
        (
            "POST",
            "presence-models.lobby",
            "42",
            200,
            json!({"member":{"room":"lobby"}}),
        ),
        ("GET", "presence-models.other", "42", 403, Value::Null),
        ("GET", "presence-models.missing", "42", 403, Value::Null),
        (
            "GET",
            "presence-models.lobby",
            "anonymous",
            403,
            Value::Null,
        ),
        (
            "GET",
            "private-explicit.key",
            "42",
            200,
            json!({"authorized":true}),
        ),
        ("GET", "private-explicit.missing", "42", 404, Value::Null),
        (
            "GET",
            "private-overlap.allow",
            "42",
            200,
            json!({"authorized":true}),
        ),
        ("GET", "private-overlap.deny", "42", 403, Value::Null),
    ] {
        let response = client
            .request(
                method.parse().unwrap(),
                format!("http://{address}/broadcasting/auth?channel_name={channel}"),
            )
            .header("Accept", "application/json")
            .header("x-identity", identity)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            status,
            "{method} {channel} {identity}"
        );
        if status == 200 {
            assert_eq!(response.json::<Value>().await.unwrap(), expected);
        }
    }
    server.abort();
    let status = application(Arc::default())
        .handleCommand(&["channel:list".into()])
        .await
        .unwrap();
    assert_eq!(status, 0);
}

#[tokio::test]
async fn broadcast_deliveries_respect_transaction_boundaries_and_queue_retry() {
    let recorder = Arc::new(Recorder::default());
    let mut app = application(recorder.clone());
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    let database = std::env::var("OPENRUST_BROADCAST_TEST_MYSQL_URL")
        .ok()
        .map_or_else(
            || json!({"driver":"sqlite","database":":memory:"}),
            |url| json!({"driver":"mysql","url":url}),
        );
    config.set("database.default", json!("broadcast")).unwrap();
    config
        .set("database.connections.broadcast", database)
        .unwrap();
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    create_jobs(&db).await;
    let manager = app.make::<BroadcastManager>().await.unwrap();
    assert!(Arc::ptr_eq(
        &manager.connection(None).unwrap(),
        &manager.connection(Some("probe")).unwrap()
    ));
    assert!(matches!(
        manager.connection(Some("missing")),
        Err(BroadcastException::UnknownConnection(_))
    ));
    assert!(manager.connection(Some("null")).is_ok());
    assert!(manager.connection(Some("log")).is_ok());

    let dispatcher = app.make::<Dispatcher>().await.unwrap();
    let (parts, body) = axum::http::Request::get("/")
        .header("X-Socket-ID", "123.456")
        .body(openrust::http::Body::empty())
        .unwrap()
        .into_parts();
    let request = Request::from_parts(parts, body);
    dispatcher
        .dispatch(Arc::new(Notice::<1>(false)), &mut EventContext::new())
        .await
        .unwrap();
    assert!(recorder.delivered.lock().unwrap().is_empty());

    let mut transaction = db.beginTransaction().await.unwrap();
    let mut context = EventContext::in_transaction(&mut transaction).toOthers(&request);
    assert!(matches!(
        dispatcher
            .dispatch(Arc::new(Notice::<1>(true)), &mut context)
            .await
            .unwrap(),
        DispatchResult::Deferred
    ));
    dispatcher
        .dispatch(Arc::new(Notice::<2>(true)), &mut context)
        .await
        .unwrap();
    transaction.rollBack(None).await.unwrap();
    assert!(recorder.delivered.lock().unwrap().is_empty());

    let queue = app
        .make::<QueueManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 0);
    let mut transaction = db.beginTransaction().await.unwrap();
    let mut context = EventContext::in_transaction(&mut transaction).toOthers(&request);
    dispatcher
        .dispatch(Arc::new(Notice::<1>(true)), &mut context)
        .await
        .unwrap();
    dispatcher
        .dispatch(Arc::new(Notice::<2>(true)), &mut context)
        .await
        .unwrap();
    assert!(recorder.delivered.lock().unwrap().is_empty());
    transaction.commit().await.unwrap();
    let channels = vec![
        "news".into(),
        "private-orders.42".into(),
        "presence-rooms.42".into(),
    ];
    let payload = BroadcastPayload::from_iter([("id".into(), json!(42))]);
    assert_eq!(
        *recorder.delivered.lock().unwrap(),
        vec![(
            channels.clone(),
            "notice".into(),
            payload.clone(),
            Some("123.456".into())
        )]
    );
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 1);
    assert_eq!(queue.size(Some("default")).await.unwrap(), 0);
    recorder.fail_once.store(true, Ordering::SeqCst);
    let mut job = queue.pop(Some("broadcasts")).await.unwrap().unwrap();
    assert!(job.fire(&mut JobContext::default()).await.is_err());
    assert_eq!(job.attempts(), 1);
    job.release(Duration::ZERO).await.unwrap();
    let arguments: Vec<_> = ["queue:work", "--queue=broadcasts", "--once", "--tries=2"]
        .into_iter()
        .map(OsString::from)
        .collect();
    assert_eq!(app.handleCommand(&arguments).await.unwrap(), 0);
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 0);
    assert_eq!(
        recorder.delivered.lock().unwrap()[1],
        (
            channels,
            "notice.queued".into(),
            payload,
            Some("123.456".into())
        )
    );
}

async fn create_jobs(db: &openrust::database::Connection) {
    db.getSchemaBuilder()
        .create("jobs", |table| {
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
}

async fn artisan_work(recorder: Arc<Recorder>, root: &Path, selected: &str, tries: u8) {
    let arguments = [
        OsString::from("queue:work"),
        OsString::from(format!("--queue={selected}")),
        OsString::from("--once"),
        OsString::from(format!("--tries={tries}")),
    ];
    assert_eq!(
        application_at(recorder, root)
            .handleCommand(&arguments)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn actual_http_and_artisan_deliver_anonymous_and_unique_broadcasts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::File::create(root.path().join("broadcast-probe.sqlite")).unwrap();
    let recorder = Arc::new(Recorder::default());
    let mut kernel = Kernel::new(application_at(recorder.clone(), root.path()));
    kernel.bootstrap().await.unwrap();
    let app = kernel.get_application();
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    create_jobs(&db).await;
    let queue = app
        .make::<QueueManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let fake = app
        .make::<Dispatcher>()
        .await
        .unwrap()
        .fake(&[UniqueJobSkipped::NAME]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let get = |path: &str| {
        let request = client
            .get(format!("http://{address}/broadcast/{path}"))
            .header("X-Socket-ID", "123.456");
        async move {
            assert_eq!(
                request.send().await.unwrap().status(),
                StatusCode::NO_CONTENT
            );
        }
    };

    get("anonymous-now").await;
    assert_eq!(
        *recorder.delivered.lock().unwrap(),
        vec![(
            vec!["news".into(), "private-alerts".into()],
            "anonymous.ready".into(),
            BroadcastPayload::from_iter([("ok".into(), json!(true))]),
            Some("123.456".into()),
        )]
    );
    assert_eq!(queue.size(Some("default")).await.unwrap(), 0);
    get("anonymous").await;
    assert_eq!(queue.size(Some("default")).await.unwrap(), 1);
    artisan_work(recorder.clone(), root.path(), "default", 1).await;
    assert_eq!(
        recorder.delivered.lock().unwrap()[1],
        (
            vec!["presence-lobby".into()],
            "AnonymousEvent".into(),
            BroadcastPayload::new(),
            None
        )
    );

    get("rollback/same").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 0);
    get("commit/same").await;
    get("unique/same").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 1);
    let skipped = fake.dispatched(UniqueJobSkipped::NAME);
    let job = &skipped[0]
        .as_ref()
        .downcast_ref::<UniqueJobSkipped>()
        .unwrap()
        .job;
    assert_eq!(job.data()["event"], "unique.notice");
    assert_eq!(job.data()["payload"]["id"], "same");
    assert_eq!(skipped.len(), 1);

    recorder.fail_once.store(true, Ordering::SeqCst);
    artisan_work(recorder.clone(), root.path(), "broadcasts", 2).await;
    get("unique/same").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 1);
    assert_eq!(fake.dispatched(UniqueJobSkipped::NAME).len(), 2);
    artisan_work(recorder.clone(), root.path(), "broadcasts", 2).await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 0);

    tokio::join!(get("unique/same"), get("unique/same"));
    assert_eq!(fake.dispatched(UniqueJobSkipped::NAME).len(), 3);
    get("unique-other/same").await;
    get("unique/another").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 3);
    recorder.fail_once.store(true, Ordering::SeqCst);
    artisan_work(recorder.clone(), root.path(), "broadcasts", 1).await;
    get("unique/same").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 3);
    for _ in 0..3 {
        artisan_work(recorder.clone(), root.path(), "broadcasts", 1).await;
    }
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 0);

    get("unique/expiring").await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    get("unique/expiring").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 2);
    artisan_work(recorder.clone(), root.path(), "broadcasts", 1).await;
    get("unique/expiring").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 1);
    artisan_work(recorder.clone(), root.path(), "broadcasts", 1).await;

    get("unique-now/same").await;
    get("unique-now/same").await;
    assert_eq!(queue.size(Some("broadcasts")).await.unwrap(), 0);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    let delivered = recorder.delivered.lock().unwrap();
    assert_eq!(delivered.len(), 10);
    assert!(
        delivered[8..]
            .iter()
            .all(|delivery| delivery.2["kind"] == 3)
    );
}
