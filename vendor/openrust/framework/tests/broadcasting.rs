use std::{
    ffi::OsString,
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
        BroadcastResult, Channel, ChannelAuthorization, ChannelOptions, ChannelParameters,
        PresenceChannel, PrivateChannel,
    },
    contracts::{
        auth::Authenticatable,
        broadcasting::{Broadcaster, Factory, ShouldBroadcast, ShouldBroadcastNow},
        events::{Dispatcher as _, Event, ShouldDispatchAfterCommit},
        foundation::Application as _,
        http::Kernel as _,
    },
    database::DatabaseManager,
    events::{DispatchResult, Dispatcher, EventContext},
    foundation::{Application, http::Kernel},
    http::{Request, Response, StatusCode},
    queue::{JobContext, QueueManager},
    support::{ProviderResult, ServiceProvider, facades::Broadcast},
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
                ChannelAuthorization::Denied => return Err(BroadcastException::AccessDenied),
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
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::String("42".into())
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
            let dispatcher = app.make::<Dispatcher>().await?;
            dispatcher.registerBroadcastNow::<Notice<1>>();
            dispatcher.markAfterCommit::<Notice<1>>();
            dispatcher.registerBroadcast::<Notice<2>>();
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

fn channels() -> BroadcastResult<()> {
    let options = || ChannelOptions {
        guards: vec!["header".into()],
    };
    Broadcast::channelWithOptions(
        "orders.{id}",
        |user: &dyn Authenticatable, parameters: &ChannelParameters| {
            Ok(
                if user.get_auth_identifier() == AuthIdentifier::String(parameters["id"].clone()) {
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
    )
}

fn application(recorder: Arc<Recorder>) -> Application {
    Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([
                ("app".into(), json!({"key": "x".repeat(32)})),
                ("auth".into(), json!({"guards":{"header":{"driver":"header"}}})),
                ("session".into(), json!({"driver":"array","lottery":[0,100]})),
                ("broadcasting".into(), json!({"default":"probe","connections":{"probe":{"driver":"record"}}})),
                ("queue".into(), json!({"default":"db","connections":{"db":{"driver":"database","queue":"default","table":"jobs","retry_after":30}}})),
            ].into())
        })
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
    let mut kernel = Kernel::new(application(Arc::default()));
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
