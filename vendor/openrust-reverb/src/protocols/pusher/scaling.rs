use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt as _;
use redis::{
    Client,
    aio::{ConnectionManager, PubSub},
};
use serde::{Deserialize, Serialize};
use tokio::time;
use tokio_util::sync::CancellationToken;

use super::channels::{Frame, Member, Metrics};

pub(super) const PRESENCE_SWEEP: Duration = Duration::from_secs(2);
const LEASE: Duration = Duration::from_secs(10);
const PRESENCE_COMMAND: &str = include_str!("presence.lua");

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Delivery {
    pub(super) app_id: String,
    pub(super) channels: Vec<String>,
    pub(super) event: String,
    pub(super) data: String,
    pub(super) excluded: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) user_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Message {
    Event(Delivery),
    Presence(PresenceDelivery),
    Metrics(MetricRequest),
}

#[derive(Deserialize)]
pub(super) struct PresenceDelivery {
    pub(super) app_id: String,
    pub(super) frame: Frame,
    pub(super) recipient: Option<String>,
    pub(super) excluded: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct MetricRequest {
    id: u64,
    pub(super) app_id: String,
    pub(super) selected: Vec<String>,
    pub(super) prefix: Option<String>,
}

pub(super) struct Subscriber {
    client: Client,
    connection: PubSub,
    channel: String,
    timeout: Duration,
    subscribed: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(super) struct Publisher {
    client: Client,
    connection: ConnectionManager,
    channel: String,
    timeout: Duration,
    subscribed: Arc<AtomicBool>,
}

impl Subscriber {
    pub(super) async fn connect(
        url: &str,
        channel: String,
        timeout: Duration,
    ) -> io::Result<(Self, Publisher)> {
        if channel.is_empty() || timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid Reverb Redis channel or timeout.",
            ));
        }
        let client = Client::open(url).map_err(|_| redis_error())?;
        let connection = subscribe(&client, &channel, timeout).await?;
        let publisher = time::timeout(timeout, ConnectionManager::new(client.clone()))
            .await
            .map_err(|_| redis_error())?
            .map_err(|_| redis_error())?;
        let subscribed = Arc::new(AtomicBool::new(true));
        Ok((
            Self {
                client: client.clone(),
                connection,
                channel: channel.clone(),
                timeout,
                subscribed: subscribed.clone(),
            },
            Publisher {
                client,
                connection: publisher,
                channel,
                timeout,
                subscribed,
            },
        ))
    }

    pub(super) async fn run(
        mut self,
        stop: CancellationToken,
        publisher: Publisher,
        deliver: impl Fn(Message),
        metrics: impl Fn(&MetricRequest) -> Metrics,
        disconnected: impl Fn(),
    ) {
        loop {
            let mut messages = self.connection.on_message();
            loop {
                let message = tokio::select! {
                    biased;
                    () = stop.cancelled() => return,
                    message = messages.next() => message,
                };
                let Some(message) = message else { break };
                if let Ok(delivery) = serde_json::from_slice::<Message>(message.get_payload_bytes())
                {
                    match delivery {
                        Message::Metrics(request) => {
                            let response = metrics(&request);
                            let reply = publisher.metric_reply(request.id);
                            tokio::select! {
                                () = stop.cancelled() => return,
                                _ = publisher.publish_to(&reply, &response) => (),
                            }
                        }
                        delivery => deliver(delivery),
                    }
                }
            }
            drop(messages);
            self.subscribed.store(false, Ordering::Release);
            disconnected();

            loop {
                let reconnect = async {
                    time::sleep(Duration::from_millis(500)).await;
                    subscribe(&self.client, &self.channel, self.timeout).await
                };
                let connection = tokio::select! {
                    biased;
                    () = stop.cancelled() => return,
                    connection = reconnect => connection,
                };
                if let Ok(connection) = connection {
                    self.connection = connection;
                    self.subscribed.store(true, Ordering::Release);
                    break;
                }
            }
        }
    }
}

impl Publisher {
    pub(super) async fn publish(&self, delivery: &Delivery) -> io::Result<()> {
        if self.publish_to(&self.channel, delivery).await? == 0 {
            return Err(redis_error());
        }
        Ok(())
    }

    async fn publish_to(&self, channel: &str, message: &impl Serialize) -> io::Result<usize> {
        let bytes = serde_json::to_vec(message)?;
        let mut connection = self.connection.clone();
        time::timeout(
            self.timeout,
            redis::cmd("PUBLISH")
                .arg(channel)
                .arg(bytes)
                .query_async(&mut connection),
        )
        .await
        .map_err(|_| redis_error())?
        .map_err(|_| redis_error())
    }

    fn metric_reply(&self, id: u64) -> String {
        format!(
            "reverb:metrics:{}:{}:{id:016x}",
            self.channel.len(),
            self.channel
        )
    }

    pub(super) async fn metrics(
        &self,
        app: &str,
        selected: &[String],
        prefix: Option<&str>,
    ) -> io::Result<Metrics> {
        if !self.subscribed.load(Ordering::Acquire) {
            return Err(redis_error());
        }
        let request = MetricRequest {
            id: fastrand::u64(..),
            app_id: app.into(),
            selected: selected.into(),
            prefix: prefix.map(str::to_owned),
        };
        let mut reply =
            subscribe(&self.client, &self.metric_reply(request.id), self.timeout).await?;
        let expected = self.publish_to(&self.channel, &request).await?;
        if expected == 0 {
            return Err(redis_error());
        }
        let mut messages = reply.on_message();
        let mut combined = Metrics::default();
        let mut received = 0;
        let _ = time::timeout(self.timeout.min(Duration::from_secs(10)), async {
            while received < expected {
                let Some(message) = messages.next().await else {
                    break;
                };
                if let Ok(metrics) = serde_json::from_slice::<Metrics>(message.get_payload_bytes())
                {
                    combined.merge(metrics);
                    received += 1;
                }
            }
        })
        .await;
        if received == 0 {
            return Err(redis_error());
        }
        Ok(combined)
    }

    pub(super) async fn join(
        &self,
        app: &str,
        channel: &str,
        socket: &str,
        member: &Member,
    ) -> io::Result<()> {
        if !self.subscribed.load(Ordering::Acquire) {
            return Err(redis_error());
        }
        let member = serde_json::to_string(&(&member.user_id, member.user_info.to_string()))?;
        self.presence(app, channel, "join", socket, &member, &[])
            .await?;
        Ok(())
    }

    pub(super) async fn leave(&self, app: &str, channel: &str, socket: &str) {
        // If Redis is unavailable the socket lease expires without being renewed.
        let _ = self.presence(app, channel, "leave", socket, "", &[]).await;
    }

    pub(super) async fn refresh(
        &self,
        app: &str,
        channel: &str,
        sockets: &[String],
    ) -> io::Result<Vec<String>> {
        self.presence(app, channel, "refresh", "", "", sockets)
            .await
    }

    async fn presence(
        &self,
        app: &str,
        channel: &str,
        operation: &str,
        socket: &str,
        member: &str,
        sockets: &[String],
    ) -> io::Result<Vec<String>> {
        let scope = format!(
            "reverb:presence:{}:{}:{}:{app}:{channel}",
            self.channel.len(),
            self.channel,
            app.len()
        );
        let mut connection = self.connection.clone();
        time::timeout(
            self.timeout.min(PRESENCE_SWEEP),
            redis::cmd("EVAL")
                .arg(PRESENCE_COMMAND)
                .arg(2)
                .arg(format!("{scope}:members"))
                .arg(format!("{scope}:leases"))
                .arg(operation)
                .arg(&self.channel)
                .arg(app)
                .arg(channel)
                .arg(socket)
                .arg(member)
                .arg(LEASE.as_millis() as u64)
                .arg(sockets)
                .query_async(&mut connection),
        )
        .await
        .map_err(|_| redis_error())?
        .map_err(|_| redis_error())
    }
}

async fn subscribe(client: &Client, channel: &str, timeout: Duration) -> io::Result<PubSub> {
    time::timeout(timeout, async {
        let mut connection = client.get_async_pubsub().await?;
        connection.subscribe(channel).await?;
        Ok::<_, redis::RedisError>(connection)
    })
    .await
    .map_err(|_| redis_error())?
    .map_err(|_| redis_error())
}

fn redis_error() -> io::Error {
    io::Error::other("Reverb could not reach the Redis scaling channel.")
}
