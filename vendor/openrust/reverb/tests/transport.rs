use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use futures_util::{SinkExt as _, StreamExt as _};
use openrust_reverb::protocols::pusher::{
    server::{Application, ClientEvents, Server},
    signatures::{sign_channel, sign_request},
};
use serde_json::{Value, json};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const SECRET: &str = "probe-secret";
const ORIGIN: &str = "https://app.example.test";
const PREFIX: &str = "/socket";

struct Running {
    address: SocketAddr,
    shutdown: CancellationToken,
    server: JoinHandle<std::io::Result<()>>,
}

impl Running {
    async fn start(applications: Vec<Application>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let server = Server::new(applications, PREFIX, 2048).unwrap();
        let task = tokio::spawn(server.serve(listener, shutdown.clone()));
        Self {
            address,
            shutdown,
            server: task,
        }
    }

    async fn socket(&self, key: &str, origin: &str) -> Socket {
        let mut request = format!(
            "ws://{}{PREFIX}/app/{key}?protocol=7&client=js",
            self.address
        )
        .into_client_request()
        .unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        connect_async(request).await.unwrap().0
    }

    async fn publish(
        &self,
        id: &str,
        key: &str,
        body: &str,
        change_body: bool,
    ) -> reqwest::Response {
        let path = format!("/apps/{id}/events");
        let mut query = BTreeMap::from([
            ("auth_key".into(), key.to_owned()),
            ("auth_timestamp".into(), "1700000000".into()),
            ("auth_version".into(), "1.0".into()),
        ]);
        let signature = sign_request(SECRET.as_bytes(), "POST", &path, &query, body.as_bytes());
        query.insert("auth_signature".into(), signature);
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .post(format!("http://{}{PREFIX}{path}", self.address))
            .query(&query)
            .body(if change_body {
                "{}".to_owned()
            } else {
                body.to_owned()
            })
            .send()
            .await
            .unwrap()
    }

    async fn stop(self) {
        self.shutdown.cancel();
        timeout(Duration::from_secs(3), self.server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(TcpListener::bind(self.address).await.is_ok());
    }
}

fn application(id: &str, key: &str) -> Application {
    Application {
        id: id.into(),
        key: key.into(),
        secret: SECRET.into(),
        allowed_origins: vec!["app.example.test".into()],
        activity_timeout: Duration::from_secs(30),
        ping_interval: Duration::from_secs(60),
        max_connections: None,
        max_message_size: 4096,
        client_events: ClientEvents::Members,
    }
}

async fn next(socket: &mut Socket) -> Value {
    let message = timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

async fn expected(socket: &mut Socket, name: &str) -> Value {
    let frame = next(socket).await;
    assert_eq!(frame["event"], name, "{frame}");
    serde_json::from_str(frame["data"].as_str().unwrap()).unwrap()
}

async fn hello(socket: &mut Socket) -> String {
    let data = expected(socket, "pusher:connection_established").await;
    assert_eq!(data["activity_timeout"], 30);
    data["socket_id"].as_str().unwrap().to_owned()
}

async fn send(socket: &mut Socket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn subscribe(socket: &mut Socket, id: &str, channel: &str, member: Option<Value>) -> Value {
    let member = member.map(|value| value.to_string());
    let auth = format!(
        "test-key:{}",
        sign_channel(SECRET.as_bytes(), id, channel, member.as_deref())
    );
    send(socket, json!({"event":"pusher:subscribe","data":{"channel":channel,"auth":auth,"channel_data":member}})).await;
    expected(socket, "pusher_internal:subscription_succeeded").await
}

async fn assert_quiet(socket: &mut Socket) {
    assert!(
        timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn serves_signed_http_and_websocket_with_reconnect_and_application_isolation() {
    let run = Running::start(vec![
        application("test", "test-key"),
        application("other", "other-key"),
    ])
    .await;
    let mut first = run.socket("test-key", ORIGIN).await;
    let first_id = hello(&mut first).await;
    let mut second = run.socket("test-key", ORIGIN).await;
    let second_id = hello(&mut second).await;
    let mut other = run.socket("other-key", ORIGIN).await;
    hello(&mut other).await;

    for socket in [&mut first, &mut second, &mut other] {
        send(
            socket,
            json!({"event":"pusher:subscribe","data":{"channel":"news"}}),
        )
        .await;
        expected(socket, "pusher_internal:subscription_succeeded").await;
    }
    let body = json!({"name":"updated","data":"{\"status\":1}","channels":["news","news"],"socket_id":first_id}).to_string();
    assert_eq!(
        run.publish("test", "test-key", &body, true).await.status(),
        401
    );
    assert_eq!(
        run.publish("test", "other-key", &body, false)
            .await
            .status(),
        401
    );
    assert_eq!(
        run.publish("test", "test-key", &body, false).await.status(),
        200
    );
    assert_eq!(expected(&mut second, "updated").await, json!({"status":1}));
    assert_quiet(&mut first).await;
    assert_quiet(&mut second).await;
    assert_quiet(&mut other).await;

    send(&mut first, json!({"event":"pusher:subscribe","data":{"channel":"private-orders","auth":"test-key:bad"}})).await;
    assert_eq!(expected(&mut first, "pusher:error").await["code"], 4009);
    subscribe(&mut first, &first_id, "private-orders", None).await;
    subscribe(&mut second, &second_id, "private-orders", None).await;
    send(
        &mut first,
        json!({"event":"client-typing","channel":"private-orders","data":{"typing":true}}),
    )
    .await;
    assert_eq!(
        expected(&mut second, "client-typing").await,
        json!({"typing":true})
    );

    let channel = "presence-room";
    subscribe(
        &mut first,
        &first_id,
        channel,
        Some(json!({"user_id":"a","user_info":{"name":"A"}})),
    )
    .await;
    let presence = subscribe(
        &mut second,
        &second_id,
        channel,
        Some(json!({"user_id":"b","user_info":{"name":"B"}})),
    )
    .await;
    assert_eq!(presence["presence"]["count"], 2);
    assert_eq!(
        expected(&mut first, "pusher_internal:member_added").await["user_id"],
        "b"
    );

    second.close(None).await.unwrap();
    assert_eq!(
        expected(&mut first, "pusher_internal:member_removed").await["user_id"],
        "b"
    );
    let mut reconnected = run.socket("test-key", ORIGIN).await;
    let new_id = hello(&mut reconnected).await;
    assert_ne!(new_id, second_id);
    send(&mut reconnected, json!({"event":"pusher:ping"})).await;
    expected(&mut reconnected, "pusher:pong").await;

    run.stop().await;
    assert!(matches!(
        timeout(Duration::from_secs(2), first.next()).await.unwrap(),
        Some(Ok(Message::Close(_))) | None
    ));
}

#[tokio::test]
async fn denies_invalid_origins_and_enforces_connection_limit() {
    let mut app = application("test", "test-key");
    app.max_connections = Some(1);
    let run = Running::start(vec![app]).await;
    let mut denied = run.socket("test-key", "https://other.example.test").await;
    assert_eq!(expected(&mut denied, "pusher:error").await["code"], 4009);
    let mut first = run.socket("test-key", ORIGIN).await;
    hello(&mut first).await;
    let mut over_limit = run.socket("test-key", ORIGIN).await;
    assert_eq!(
        expected(&mut over_limit, "pusher:error").await["code"],
        4004
    );
    run.stop().await;
}

struct User;

impl openrust::contracts::auth::Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        openrust::auth::AuthIdentifier::String("0042".into())
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

struct RegisterGuard;

impl openrust::support::ServiceProvider for RegisterGuard {
    fn boot<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            app.make::<openrust::auth::AuthManager>()
                .await?
                .viaRequest("reverb-test", |request| {
                    Box::pin(async move {
                        Ok(request
                            .headers()
                            .get("x-identity")
                            .is_some_and(|value| value == "0042")
                            .then(|| {
                                Box::new(User)
                                    as Box<dyn openrust::contracts::auth::Authenticatable>
                            }))
                    })
                });
            Ok(())
        })
    }
}

fn room_channel() -> openrust::broadcasting::BroadcastResult<()> {
    use openrust::{
        broadcasting::{ChannelAuthorization, ChannelOptions, ChannelParameters},
        contracts::auth::Authenticatable,
        support::facades::Broadcast,
    };
    Broadcast::channelWithOptions(
        "room.{id}",
        |_: &dyn Authenticatable, parameters: &ChannelParameters| {
            Ok(ChannelAuthorization::Presence(
                [
                    ("name".into(), json!("member")),
                    ("room".into(), json!(parameters["id"])),
                ]
                .into_iter()
                .collect(),
            ))
        },
        ChannelOptions {
            guards: vec!["test".into()],
        },
    )
}

async fn framework_http(
    axum::extract::State(kernel): axum::extract::State<
        std::sync::Arc<openrust::foundation::http::Kernel>,
    >,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

#[tokio::test]
async fn framework_broadcaster_signs_guard_identity_and_delivers_to_real_client() {
    use axum::handler::Handler as _;
    use openrust::{
        broadcasting::{BroadcastChannel, BroadcastManager, PresenceChannel},
        contracts::{broadcasting::Factory as _, foundation::Application as _, http::Kernel as _},
        foundation::{Application as Framework, http::Kernel},
    };

    let reverb = Running::start(vec![application("test", "test-key")]).await;
    let mut app = Framework::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "auth".into(),
                json!({"guards":{"test":{"driver":"reverb-test"}}}),
            )]
            .into())
        })
        .withBroadcasting(room_channel)
        .withProviders(vec![
            Box::new(openrust_reverb::ReverbServiceProvider),
            Box::new(RegisterGuard),
        ])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    config.set("broadcasting.default", json!("reverb")).unwrap();
    config.set("broadcasting.connections.reverb", json!({
        "driver": "reverb",
        "key": "test-key",
        "secret": SECRET,
        "app_id": "test",
        "options": {"host": "127.0.0.1", "scheme": "http", "port": reverb.address.port(), "path": PREFIX},
    })).unwrap();
    let connection = app
        .make::<BroadcastManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();

    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let framework = tokio::spawn(async move {
        axum::serve(
            listener,
            framework_http
                .with_state(std::sync::Arc::new(kernel))
                .into_make_service(),
        )
        .await
        .unwrap();
    });

    let mut websocket = reverb.socket("test-key", ORIGIN).await;
    let id = hello(&mut websocket).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let authorization = |identity: &str| {
        client
            .post(format!("http://{address}/broadcasting/auth"))
            .header("accept", "application/json")
            .header("x-identity", identity)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!(
                "socket_id={id}&channel_name=presence-room.9&user_id=forged"
            ))
    };
    assert_eq!(authorization("other").send().await.unwrap().status(), 403);
    let authorization = authorization("0042")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let data =
        serde_json::from_str::<Value>(authorization["channel_data"].as_str().unwrap()).unwrap();
    assert_eq!(data["user_id"], "0042");
    assert_eq!(data["user_info"]["room"], "9");

    send(
        &mut websocket,
        json!({"event":"pusher:subscribe","data":{
            "channel":"presence-room.9",
            "auth":authorization["auth"],
            "channel_data":authorization["channel_data"]
        }}),
    )
    .await;
    assert_eq!(
        expected(&mut websocket, "pusher_internal:subscription_succeeded").await["presence"]["ids"],
        json!(["0042"])
    );

    connection
        .broadcast(
            &[BroadcastChannel::Presence(PresenceChannel {
                name: "room.9".into(),
            })],
            "message",
            &[("id".into(), json!(9))].into_iter().collect(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(expected(&mut websocket, "message").await, json!({"id":9}));
    framework.abort();
    reverb.stop().await;
}
