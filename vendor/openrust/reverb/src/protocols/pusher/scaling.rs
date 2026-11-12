use std::{io, time::Duration};

use futures_util::StreamExt as _;
use redis::{
    Client,
    aio::{ConnectionManager, PubSub},
};
use serde::{Deserialize, Serialize};
use tokio::time;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Delivery {
    pub(super) app_id: String,
    pub(super) channels: Vec<String>,
    pub(super) event: String,
    pub(super) data: String,
    pub(super) excluded: Option<String>,
}

pub(super) struct Subscriber {
    client: Client,
    connection: PubSub,
    channel: String,
    timeout: Duration,
}

#[derive(Clone)]
pub(super) struct Publisher {
    connection: ConnectionManager,
    channel: String,
    timeout: Duration,
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
        Ok((
            Self {
                client,
                connection,
                channel: channel.clone(),
                timeout,
            },
            Publisher {
                connection: publisher,
                channel,
                timeout,
            },
        ))
    }

    pub(super) async fn run(mut self, stop: CancellationToken, deliver: impl Fn(Delivery)) {
        loop {
            let mut messages = self.connection.on_message();
            loop {
                let message = tokio::select! {
                    biased;
                    () = stop.cancelled() => return,
                    message = messages.next() => message,
                };
                let Some(message) = message else { break };
                if let Ok(delivery) =
                    serde_json::from_slice::<Delivery>(message.get_payload_bytes())
                {
                    deliver(delivery);
                }
            }
            drop(messages);

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
                    break;
                }
            }
        }
    }
}

impl Publisher {
    pub(super) async fn publish(&self, delivery: &Delivery) -> io::Result<()> {
        let bytes = serde_json::to_vec(delivery)?;
        let mut connection = self.connection.clone();
        let published: usize = time::timeout(
            self.timeout,
            redis::cmd("PUBLISH")
                .arg(&self.channel)
                .arg(bytes)
                .query_async(&mut connection),
        )
        .await
        .map_err(|_| redis_error())?
        .map_err(|_| redis_error())?;
        if published == 0 {
            return Err(redis_error());
        }
        Ok(())
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
