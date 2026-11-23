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
        self.request(
            reqwest::Method::POST,
            &format!("/apps/{id}/events"),
            key,
            &[],
            body,
            if change_body { "{}" } else { body },
        )
        .await
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        key: &str,
        parameters: &[(&str, &str)],
        signed: &str,
        body: &str,
    ) -> reqwest::Response {
        let mut query = BTreeMap::from([
            ("auth_key".into(), key.to_owned()),
            ("auth_timestamp".into(), "1700000000".into()),
            ("auth_version".into(), "1.0".into()),
        ]);
        query.extend(
            parameters
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        );
        let signature = sign_request(
            SECRET.as_bytes(),
            method.as_str(),
            path,
            &query,
            signed.as_bytes(),
        );
        query.insert("auth_signature".into(), signature);
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .request(method, format!("http://{}{PREFIX}{path}", self.address))
            .query(&query)
            .body(body.to_owned())
            .send()
            .await
            .unwrap()
    }

    async fn get(&self, path: &str, parameters: &[(&str, &str)]) -> reqwest::Response {
        self.request(reqwest::Method::GET, path, "test-key", parameters, "", "")
            .await
    }

    async fn batch(&self, payload: Value) -> reqwest::Response {
        let body = payload.to_string();
        self.request(
            reqwest::Method::POST,
            "/apps/test/batch_events",
            "test-key",
            &[],
            &body,
            &body,
        )
        .await
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
                .via_request("reverb-test", |request| {
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
    Broadcast::channel_with_options(
        "orders.{id}",
        |_: &dyn Authenticatable, _: &ChannelParameters| Ok(ChannelAuthorization::Allowed),
        ChannelOptions {
            guards: vec!["test".into()],
        },
    )?;
    Broadcast::channel_with_options(
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
    kernel.handle_transport(request).await
}

struct PublishProbe(openrust::console::CommandDefinition);

impl openrust::console::Command for PublishProbe {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a openrust::foundation::Application,
        input: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        use openrust::{
            broadcasting::{BroadcastChannel, PresenceChannel, PrivateChannel},
            console::ConsoleError,
            support::facades::Broadcast,
        };
        Box::pin(async move {
            let sequence = input.matches().get_one::<u32>("sequence").unwrap();
            let channels = [
                BroadcastChannel::from("echo.news"),
                PrivateChannel {
                    name: "orders.9".into(),
                }
                .into(),
                PresenceChannel {
                    name: "room.9".into(),
                }
                .into(),
            ];
            let connection = Broadcast::connection(None)
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            connection
                .broadcast(
                    &channels,
                    "echo.probe",
                    &[("sequence".into(), json!(sequence))].into_iter().collect(),
                    None,
                )
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            Ok(0)
        })
    }
}

fn register_probe(
    artisan: &mut openrust::console::Application,
) -> Result<(), openrust::console::ConsoleError> {
    use openrust::console::CommandDefinition;
    let command = clap::Command::new("pusher:probe").arg(
        clap::Arg::new("sequence")
            .required(true)
            .value_parser(clap::value_parser!(u32)),
    );
    artisan.add_command(Box::new(PublishProbe(CommandDefinition::new(command))))
}

async fn framework_application(
    root: &std::path::Path,
    port: u16,
    driver: &str,
) -> openrust::foundation::Application {
    use openrust::{contracts::foundation::Application as _, foundation::Application};
    let mut app = Application::configure(root.to_path_buf())
        .with_config(|_, _| {
            Ok([(
                "auth".into(),
                json!({"guards":{"test":{"driver":"reverb-test"}}}),
            )]
            .into())
        })
        .with_broadcasting(room_channel)
        .with_commands(register_probe)
        .with_providers(vec![
            Box::new(openrust_reverb::ReverbServiceProvider),
            Box::new(RegisterGuard),
        ])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    config.set("broadcasting.default", json!(driver)).unwrap();
    config
        .set(
            &format!("broadcasting.connections.{driver}"),
            json!({
                "driver": driver,
                "key": "test-key",
                "secret": SECRET,
                "app_id": "test",
                "options": {"host": "127.0.0.1", "scheme": "http", "port": port, "path": PREFIX},
            }),
        )
        .unwrap();
    app
}

struct TlsProxy(std::process::Child);

impl TlsProxy {
    fn start(port: u16) -> (Self, u16) {
        use std::io::BufRead;
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/reverb_tls_acceptance.py");
        let mut child = Self(
            std::process::Command::new("python3")
                .args(["-B", "-u"])
                .arg(script)
                .args(["--proxy", &port.to_string()])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut address = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let port = address.trim().parse().expect("TLS proxy did not start");
        (child, port)
    }
}

impl Drop for TlsProxy {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn configure_tls_publishing(app: &openrust::foundation::Application, port: u16) {
    use openrust::contracts::foundation::Application as _;
    app.config()
        .unwrap()
        .set(
            "broadcasting.connections.pusher.options",
            json!({"host":"localhost", "scheme":"https", "port":port, "path":PREFIX}),
        )
        .unwrap();
    app.config()
        .unwrap()
        .set(
            "broadcasting.connections.pusher.client_options.verify",
            json!(std::env::var("OPENRUST_REVERB_TLS_CA").unwrap()),
        )
        .unwrap();
}

#[tokio::test]
async fn framework_broadcaster_signs_guard_identity_and_delivers_to_real_client() {
    verify_framework_broadcast("reverb").await;
}

#[tokio::test]
async fn named_pusher_connection_authorizes_and_publishes_through_artisan() {
    verify_framework_broadcast("pusher").await;
}

async fn verify_framework_broadcast(driver: &'static str) {
    use openrust::{
        broadcasting::{BroadcastChannel, BroadcastManager, BroadcastPayload, PresenceChannel},
        contracts::{broadcasting::Factory as _, http::Kernel as _},
        foundation::http::Kernel,
    };

    let mut configuration = application("test", "test-key");
    configuration.allowed_origins.push("127.0.0.1".into());
    configuration.allowed_origins.push("localhost".into());
    let reverb = Running::start(vec![configuration]).await;
    let root = std::env::temp_dir();
    let reverb_port = reverb.address.port();
    let tls = (driver == "pusher" && std::env::var_os("OPENRUST_REVERB_TLS_CERT").is_some())
        .then(|| TlsProxy::start(reverb_port));
    let publish_port = tls.as_ref().map(|(_, port)| *port);
    let app = framework_application(&root, reverb_port, driver).await;
    if let Some(port) = publish_port {
        configure_tls_publishing(&app, port).await;
    }
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
    let test_root = root.clone();
    let router = axum::Router::new()
        .route(
            "/fixture/publish/{sequence}",
            axum::routing::post(
                move |axum::extract::Path(sequence): axum::extract::Path<u32>| {
                    let test_root = test_root.clone();
                    async move {
                        let artisan = framework_application(&test_root, reverb_port, driver).await;
                        if let Some(port) = publish_port {
                            configure_tls_publishing(&artisan, port).await;
                        }
                        match artisan
                            .handle_command(&["pusher:probe".into(), sequence.to_string().into()])
                            .await
                        {
                            Ok(0) => axum::http::StatusCode::NO_CONTENT,
                            _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        }
                    }
                },
            ),
        )
        .fallback(framework_http)
        .with_state(std::sync::Arc::new(kernel));
    let framework = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
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
    if let Some(port) = publish_port {
        use openrust::contracts::foundation::Application as _;
        for failure in ["default", "wrong-ca", "hostname"] {
            let rejected = framework_application(&root, reverb_port, driver).await;
            configure_tls_publishing(&rejected, port).await;
            let config = rejected.config().unwrap();
            match failure {
                "default" => config
                    .set(
                        "broadcasting.connections.pusher.client_options.verify",
                        json!(true),
                    )
                    .unwrap(),
                "wrong-ca" => config
                    .set(
                        "broadcasting.connections.pusher.client_options.verify",
                        json!(std::env::var("OPENRUST_REVERB_TLS_WRONG_CA").unwrap()),
                    )
                    .unwrap(),
                _ => config
                    .set(
                        "broadcasting.connections.pusher.options.host",
                        json!("127.0.0.1"),
                    )
                    .unwrap(),
            }
            let broadcaster = rejected
                .make::<BroadcastManager>()
                .await
                .unwrap()
                .connection(None)
                .unwrap();
            let result = broadcaster
                .broadcast(
                    &[BroadcastChannel::Presence(PresenceChannel {
                        name: "room.9".into(),
                    })],
                    "must-not-deliver",
                    &BroadcastPayload::new(),
                    None,
                )
                .await;
            assert!(result.is_err(), "TLS {failure} must reject the request");
        }
        assert!(
            timeout(Duration::from_millis(100), websocket.next())
                .await
                .is_err()
        );
        println!(
            "Rust Pusher HTTPS: trusted CA delivered; default trust, wrong CA and hostname rejected"
        );
    }
    if driver == "pusher" {
        let response = client
            .post(format!("http://{address}/fixture/publish/1"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
        assert_eq!(
            expected(&mut websocket, "echo.probe").await,
            json!({"sequence":1})
        );
    }
    if std::env::var_os("OPENRUST_REVERB_ECHO_MODULES").is_some() {
        let endpoint = format!("http://{}", reverb.address);
        let auth = format!("http://{address}");
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/reverb_echo.mjs");
        let result = tokio::task::spawn_blocking(move || {
            std::process::Command::new("node")
                .arg(script)
                .args([&auth, &endpoint, PREFIX, driver])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            result.status.success(),
            "Echo acceptance failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        print!("{}", String::from_utf8_lossy(&result.stdout));
    }
    framework.abort();
    reverb.stop().await;
}

#[tokio::test]
async fn signed_rest_metrics_batch_and_cache_channels_follow_reverb() {
    let run = Running::start(vec![
        application("test", "test-key"),
        application("other", "other-key"),
    ])
    .await;
    let mut first = run.socket("test-key", ORIGIN).await;
    let first_id = hello(&mut first).await;
    let mut next_client = run.socket("test-key", ORIGIN).await;
    let next_id = hello(&mut next_client).await;
    let mut other = run.socket("other-key", ORIGIN).await;
    hello(&mut other).await;
    let cache = "cache-news";
    let private = "private-cache-draft";
    let presence = "presence-cache-room";
    let member = json!({"user_id":"007","user_info":{"name":"First"}});
    for (channel, member) in [
        (cache, None),
        (private, None),
        (presence, Some(member.clone())),
    ] {
        subscribe(&mut first, &first_id, channel, member).await;
        assert_eq!(
            next(&mut first).await,
            json!({"event":"pusher:cache_miss","channel":channel})
        );
    }
    for channel in [cache, private, presence] {
        assert_eq!(
            run.publish(
                "test",
                "test-key",
                &json!({"name":"ready","data":"{}","channel":channel}).to_string(),
                false
            )
            .await
            .status(),
            200
        );
        assert_eq!(expected(&mut first, "ready").await, json!({}));
    }
    for (channel, member) in [(cache, None), (private, None), (presence, Some(member))] {
        subscribe(&mut next_client, &next_id, channel, member).await;
        assert_eq!(expected(&mut next_client, "ready").await, json!({}));
    }

    assert_eq!(
        run.get("/apps/test/connections", &[])
            .await
            .json::<Value>()
            .await
            .unwrap(),
        json!({"connections":2})
    );
    let wrong = run
        .request(
            reqwest::Method::GET,
            "/apps/test/connections",
            "other-key",
            &[],
            "",
            "",
        )
        .await;
    assert_eq!(wrong.status(), 401);
    let channels = run
        .get(
            "/apps/test/channels",
            &[
                ("filter_by_prefix", "cache-"),
                ("info", "subscription_count,cache"),
            ],
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        channels,
        json!({"channels":{"cache-news":{"subscription_count":2,"cache":{"event":"ready","channel":cache,"data":"{}"}}}})
    );
    let info = run
        .get(
            "/apps/test/channels/presence-cache-room",
            &[("info", "user_count,subscription_count")],
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(info, json!({"occupied":true,"user_count":1}));
    assert_eq!(
        run.get("/apps/test/channels/presence-cache-room/users", &[])
            .await
            .json::<Value>()
            .await
            .unwrap(),
        json!({"users":[{"id":"007"}]})
    );
    assert_eq!(
        run.get("/apps/test/channels/cache-news/users", &[])
            .await
            .status(),
        400
    );
    assert_eq!(
        run.get("/apps/test/channels/missing/users", &[])
            .await
            .status(),
        404
    );
    assert_eq!(
        run.get(
            "/apps/test/channels/missing",
            &[("info", "subscription_count")]
        )
        .await
        .json::<Value>()
        .await
        .unwrap(),
        json!({"occupied":false})
    );

    let invalid = run
        .batch(json!({"batch":[
            {"name":"no-delivery","data":"{}","channel":cache},
            {"name":"invalid","data":{},"channel":cache}
        ]}))
        .await;
    assert_eq!(invalid.status(), 422);
    assert_quiet(&mut first).await;
    assert_quiet(&mut next_client).await;
    let response = run.batch(json!({"batch":[
        {"name":"one","data":r#"{"sequence":1}"#,"channel":cache,"socket_id":first_id,"info":"subscription_count"},
        {"name":"two","data":r#"{"sequence":2}"#,"channel":cache},
        {"name":"three","data":"{}","channel":presence,"info":"user_count"}
    ]})).await;
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"batch":[{"subscription_count":2},{},{"user_count":1}]})
    );
    assert_eq!(expected(&mut first, "two").await, json!({"sequence":2}));
    assert_eq!(
        expected(&mut next_client, "one").await,
        json!({"sequence":1})
    );
    assert_eq!(
        expected(&mut next_client, "two").await,
        json!({"sequence":2})
    );
    expected(&mut first, "three").await;
    expected(&mut next_client, "three").await;
    assert_quiet(&mut other).await;
    let event = run.publish("test", "test-key", &json!({"name":"snapshot","data":"{}","channels":[cache,"missing"],"info":"occupied,subscription_count"}).to_string(), false).await;
    assert_eq!(
        event.json::<Value>().await.unwrap(),
        json!({"channels":{(cache):{"occupied":true,"subscription_count":2},"missing":{"occupied":false}}})
    );
    let empty = run
        .batch(json!({"batch":[{"name":"silent","data":"{}","channel":"empty"}]}))
        .await;
    assert_eq!(empty.json::<Value>().await.unwrap(), json!({"batch":{}}));

    first.close(None).await.unwrap();
    next_client.close(None).await.unwrap();
    let mut cold = run.socket("test-key", ORIGIN).await;
    let cold_id = hello(&mut cold).await;
    for _ in 0..30 {
        let result = run
            .get("/apps/test/channels/cache-news", &[])
            .await
            .json::<Value>()
            .await
            .unwrap();
        if result == json!({"occupied":false}) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    subscribe(&mut cold, &cold_id, cache, None).await;
    assert_eq!(next(&mut cold).await["event"], "pusher:cache_miss");
    run.stop().await;
}
