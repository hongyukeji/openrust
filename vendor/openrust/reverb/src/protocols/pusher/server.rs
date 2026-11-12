use std::{
    collections::{BTreeMap, HashMap},
    io,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Path, RawQuery, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::ORIGIN},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::mpsc,
    time::{self, Instant},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    channels::{ChannelBroker, Frame, Member},
    scaling::{Delivery, Publisher, Subscriber},
    signatures,
};

const SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// Runtime policy and credentials for one Reverb application.
pub struct Application {
    pub id: String,
    pub key: String,
    pub secret: String,
    pub allowed_origins: Vec<String>,
    pub activity_timeout: Duration,
    pub ping_interval: Duration,
    pub max_connections: Option<usize>,
    pub max_message_size: usize,
    pub client_events: ClientEvents,
}

#[derive(Clone, Copy)]
pub enum ClientEvents {
    Disabled,
    Members,
    All,
}

struct App {
    configuration: Application,
    broker: ChannelBroker,
}

struct ServerState {
    ids: HashMap<String, Arc<App>>,
    keys: HashMap<String, Arc<App>>,
    shutdown: CancellationToken,
    sockets: TaskTracker,
    publisher: Option<Publisher>,
}

/// Pusher-compatible Reverb listener for configured applications.
pub struct Server {
    state: Arc<ServerState>,
    prefix: String,
    max_request_size: usize,
    subscriber: Option<Subscriber>,
}

impl Server {
    pub fn new(
        applications: impl IntoIterator<Item = Application>,
        prefix: &str,
        max_request_size: usize,
    ) -> io::Result<Self> {
        let prefix = prefix.trim_matches('/');
        if !prefix.is_empty()
            && !prefix.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            })
        {
            return Err(invalid("Invalid Reverb server path."));
        }
        let mut ids = HashMap::new();
        let mut keys = HashMap::new();
        for configuration in applications {
            if configuration.id.is_empty()
                || configuration.key.is_empty()
                || configuration.key.contains(':')
                || configuration.secret.is_empty()
                || configuration.ping_interval.is_zero()
                || configuration.activity_timeout.is_zero()
                || configuration.max_message_size == 0
                || ids.contains_key(&configuration.id)
                || keys.contains_key(&configuration.key)
            {
                return Err(invalid(
                    "Invalid or duplicate Reverb application configuration.",
                ));
            }
            let app = Arc::new(App {
                configuration,
                broker: ChannelBroker::default(),
            });
            ids.insert(app.configuration.id.clone(), app.clone());
            keys.insert(app.configuration.key.clone(), app);
        }
        if ids.is_empty() || max_request_size == 0 {
            return Err(invalid(
                "Reverb requires applications and a positive request size limit.",
            ));
        }
        Ok(Self {
            state: Arc::new(ServerState {
                ids,
                keys,
                shutdown: CancellationToken::new(),
                sockets: TaskTracker::new(),
                publisher: None,
            }),
            prefix: if prefix.is_empty() {
                String::new()
            } else {
                format!("/{prefix}")
            },
            max_request_size,
            subscriber: None,
        })
    }

    /// Connects this listener to a shared Redis delivery channel before accepting traffic.
    pub async fn with_redis(
        mut self,
        url: &str,
        channel: String,
        timeout: Duration,
    ) -> io::Result<Self> {
        let (subscriber, publisher) = Subscriber::connect(url, channel, timeout).await?;
        Arc::get_mut(&mut self.state)
            .expect("server state has not been shared")
            .publisher = Some(publisher);
        self.subscriber = Some(subscriber);
        Ok(self)
    }

    pub async fn serve(self, listener: TcpListener, shutdown: CancellationToken) -> io::Result<()> {
        let socket_shutdown = self.state.shutdown.clone();
        let sockets = self.state.sockets.clone();
        let stop_sockets = socket_shutdown.clone();
        let redis = self.subscriber.map(|subscriber| {
            let state = self.state.clone();
            let stop = stop_sockets.clone();
            tokio::spawn(subscriber.run(stop, move |delivery| state.deliver(delivery)))
        });
        let router = Router::new()
            .route(&format!("{}/app/{{key}}", self.prefix), get(upgrade))
            .route(
                &format!("{}/apps/{{id}}/events", self.prefix),
                post(publish),
            )
            .layer(DefaultBodyLimit::max(self.max_request_size))
            .with_state(self.state);
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
                socket_shutdown.cancel();
            })
            .await;
        stop_sockets.cancel();
        sockets.close();
        sockets.wait().await;
        if let Some(redis) = redis {
            redis.await.map_err(io::Error::other)?;
        }
        result
    }
}

impl ServerState {
    fn deliver(&self, delivery: Delivery) {
        let Some(app) = self.ids.get(&delivery.app_id) else {
            return;
        };
        for channel in delivery.channels {
            app.broker.publish(
                &channel,
                &delivery.event,
                delivery.data.clone(),
                delivery.excluded.as_deref(),
            );
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

async fn upgrade(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    let Some(app) = state.keys.get(&key).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let allowed = origin_allowed(&app.configuration, &headers);
    let active = state.sockets.token();
    websocket
        .max_message_size(app.configuration.max_message_size)
        .max_frame_size(app.configuration.max_message_size)
        .on_upgrade(move |socket| async move {
            let _active = active;
            session(socket, app, allowed, state.shutdown.clone()).await;
        })
}

fn origin_allowed(application: &Application, headers: &HeaderMap) -> bool {
    if application
        .allowed_origins
        .iter()
        .any(|origin| origin == "*")
    {
        return true;
    }
    let Some(origin) = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| url::Url::parse(value).ok())
    else {
        return false;
    };
    let Some(host) = origin.host_str() else {
        return false;
    };
    application.allowed_origins.iter().any(|allowed| {
        allowed.eq_ignore_ascii_case(host)
            || allowed.strip_prefix("*.").is_some_and(|suffix| {
                host.len() > suffix.len() + 1
                    && host[..host.len() - suffix.len()].ends_with('.')
                    && host[host.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            })
    })
}

async fn session(
    mut websocket: WebSocket,
    app: Arc<App>,
    allowed: bool,
    shutdown: CancellationToken,
) {
    if !allowed {
        send_frame(&mut websocket, error(4009, "Origin not allowed")).await;
        return;
    }
    let socket = format!("{}.{}", fastrand::u32(..), fastrand::u32(..));
    let disconnected = shutdown.child_token();
    let (send, mut receive) = mpsc::channel(64);
    if !app.broker.connect(
        socket.clone(),
        send,
        disconnected.clone(),
        app.configuration.max_connections,
    ) {
        send_frame(&mut websocket, error(4004, "Connection limit exceeded")).await;
        return;
    }

    let hello = Frame::new(
        "pusher:connection_established",
        json!({
            "socket_id": socket,
            "activity_timeout": app.configuration.activity_timeout.as_secs(),
        })
        .to_string(),
        None,
    );
    if send_frame(&mut websocket, hello).await {
        let mut deadline = Instant::now() + app.configuration.ping_interval;
        let mut waiting = false;
        loop {
            let outgoing = tokio::select! {
                biased;
                () = disconnected.cancelled() => break,
                () = time::sleep_until(deadline) => {
                    if waiting {
                        break;
                    }
                    waiting = true;
                    deadline = Instant::now() + app.configuration.activity_timeout;
                    Some(Frame::new("pusher:ping", "{}".into(), None))
                }
                incoming = websocket.recv() => {
                    let Some(Ok(message)) = incoming else { break };
                    waiting = false;
                    deadline = Instant::now() + app.configuration.ping_interval;
                    match message {
                        Message::Text(text) => client_message(&app, &socket, &text),
                        Message::Close(_) => break,
                        Message::Ping(_) | Message::Pong(_) => None,
                        Message::Binary(_) => Some(error(4200, "Invalid message format")),
                    }
                }
                outgoing = receive.recv() => {
                    let Some(outgoing) = outgoing else { break };
                    Some(outgoing)
                }
            };
            if let Some(frame) = outgoing
                && !send_frame(&mut websocket, frame).await
            {
                break;
            }
        }
    }
    app.broker.disconnect(&socket);
    let _ = time::timeout(SEND_TIMEOUT, websocket.send(Message::Close(None))).await;
}

async fn send_frame(websocket: &mut WebSocket, frame: Frame) -> bool {
    let Ok(text) = serde_json::to_string(&frame) else {
        return false;
    };
    matches!(
        time::timeout(SEND_TIMEOUT, websocket.send(Message::Text(text.into()))).await,
        Ok(Ok(()))
    )
}

#[derive(Deserialize)]
struct ClientMessage {
    event: String,
    channel: Option<String>,
    #[serde(default)]
    data: Value,
}

#[derive(Deserialize)]
struct Subscription {
    channel: String,
    auth: Option<String>,
    channel_data: Option<String>,
}

fn client_message(app: &App, socket: &str, input: &str) -> Option<Frame> {
    let Ok(message) = serde_json::from_str::<ClientMessage>(input) else {
        return Some(error(4200, "Invalid message format"));
    };
    match message.event.as_str() {
        "pusher:ping" => Some(Frame::new("pusher:pong", "{}".into(), None)),
        "pusher:pong" => None,
        "pusher:subscribe" | "pusher:unsubscribe" => {
            let subscription = match message.data {
                Value::String(data) => serde_json::from_str(&data),
                data => serde_json::from_value(data),
            };
            let Ok(subscription) = subscription else {
                return Some(error(4200, "Invalid message format"));
            };
            if message.event == "pusher:unsubscribe" {
                let subscription: Subscription = subscription;
                app.broker.unsubscribe(socket, &subscription.channel);
                None
            } else {
                subscribe(app, socket, subscription).err()
            }
        }
        event if event.starts_with("client-") => {
            client_event(app, socket, event, message.channel, message.data).err()
        }
        _ => Some(error(4200, "Invalid message format")),
    }
}

fn subscribe(app: &App, socket: &str, subscription: Subscription) -> Result<(), Frame> {
    let channel = subscription.channel.as_str();
    let data = subscription.channel_data.as_deref();
    if channel.starts_with("private-") || channel.starts_with("presence-") {
        let authenticated = subscription
            .auth
            .as_deref()
            .and_then(|auth| auth.split_once(':'))
            .is_some_and(|(key, signature)| {
                key == app.configuration.key
                    && signatures::verify_channel(
                        app.configuration.secret.as_bytes(),
                        socket,
                        channel,
                        data,
                        signature,
                    )
            });
        if !authenticated {
            return Err(error(4009, "Connection is unauthorized"));
        }
    }
    let member = if channel.starts_with("presence-") {
        let member = data
            .and_then(|data| serde_json::from_str::<Member>(data).ok())
            .ok_or_else(|| error(4200, "Invalid presence member"))?;
        Some(member)
    } else {
        None
    };
    app.broker
        .subscribe(socket, channel, member)
        .map_err(|_| error(4200, "Invalid channel subscription"))
}

fn client_event(
    app: &App,
    socket: &str,
    event: &str,
    channel: Option<String>,
    data: Value,
) -> Result<(), Frame> {
    let channel = channel.ok_or_else(|| error(4200, "Invalid message format"))?;
    let members_only = match app.configuration.client_events {
        ClientEvents::Disabled => {
            return Err(error(
                4301,
                "The app does not have client messaging enabled.",
            ));
        }
        ClientEvents::Members => true,
        ClientEvents::All => false,
    };
    let data = match data {
        Value::String(data) => data,
        data => data.to_string(),
    };
    if !app
        .broker
        .publish_client(socket, &channel, event, data, members_only)
    {
        return Err(error(
            4009,
            "The client is not a member of the specified channel.",
        ));
    }
    Ok(())
}

fn error(code: u16, message: &str) -> Frame {
    Frame::new(
        "pusher:error",
        json!({"code": code, "message": message}).to_string(),
        None,
    )
}

#[derive(Deserialize)]
struct Publish {
    name: String,
    data: String,
    channel: Option<String>,
    #[serde(default)]
    channels: Vec<String>,
    socket_id: Option<String>,
}

async fn publish(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    let Some(app) = state.ids.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut parameters = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_deref().unwrap_or("").as_bytes()) {
        if parameters
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
    }
    if parameters.get("auth_key") != Some(&app.configuration.key)
        || !signatures::verify_request(
            app.configuration.secret.as_bytes(),
            "POST",
            &format!("/apps/{id}/events"),
            &parameters,
            &body,
        )
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(mut event) = serde_json::from_slice::<Publish>(&body) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    if event.channels.is_empty()
        && let Some(channel) = event.channel
    {
        event.channels.push(channel);
    }
    if event.name.is_empty()
        || event.channels.is_empty()
        || event.data.is_empty()
        || event
            .channels
            .iter()
            .any(|channel| !super::channels::valid_channel(channel))
    {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    }
    event.channels.sort_unstable();
    event.channels.dedup();
    let delivery = Delivery {
        app_id: id,
        channels: event.channels,
        event: event.name,
        data: event.data,
        excluded: event.socket_id,
    };
    if let Some(publisher) = &state.publisher {
        if publisher.publish(&delivery).await.is_err() {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    } else {
        state.deliver(delivery);
    }
    Json(json!({})).into_response()
}
