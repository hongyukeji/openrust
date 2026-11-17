use std::time::Duration;

use ::redis::{
    AsyncConnectionConfig, Client, ConnectionInfo, FromRedisValue,
    aio::{ConnectionManager, ConnectionManagerConfig},
    cluster::{ClusterClient, ClusterConfig},
    cluster_async::ClusterConnection,
};
use tokio::sync::OnceCell;

use crate::contracts::redis::Connection as Contract;

use super::{Subscription, cluster::Cluster};
use crate::redis::{RedisError, RedisFuture, RedisResult, Value, backend};

/// A named Redis command connection with a shared reconnecting transport.
pub struct Connection {
    client: Backend,
    transport: OnceCell<Transport>,
    prefix: String,
}

enum Backend {
    Standalone {
        client: Client,
        config: ConnectionManagerConfig,
    },
    Cluster(Cluster),
}

#[derive(Clone)]
enum Transport {
    Standalone(ConnectionManager),
    Cluster(ClusterConnection),
}

impl Connection {
    pub(crate) fn new(client: Client, config: ConnectionManagerConfig, prefix: String) -> Self {
        Self {
            client: Backend::Standalone { client, config },
            transport: OnceCell::new(),
            prefix,
        }
    }

    pub(crate) fn cluster(
        client: ClusterClient,
        seed: ConnectionInfo,
        redirects: u32,
        prefix: String,
    ) -> Self {
        Self {
            client: Backend::Cluster(Cluster::new(client, seed, redirects)),
            transport: OnceCell::new(),
            prefix,
        }
    }

    pub(crate) fn is_cluster(&self) -> bool {
        matches!(self.client, Backend::Cluster(_))
    }

    pub(crate) async fn query<T: FromRedisValue>(&self, command: &::redis::Cmd) -> RedisResult<T> {
        match self.transport().await? {
            Transport::Standalone(mut transport) => command.query_async(&mut transport).await,
            Transport::Cluster(transport) => {
                let Backend::Cluster(cluster) = &self.client else {
                    unreachable!("cluster transport")
                };
                cluster.query(transport, command).await
            }
        }
        .map_err(backend)
    }

    pub(crate) async fn pipeline(&self, pipeline: &::redis::Pipeline) -> RedisResult<()> {
        match self.transport().await? {
            Transport::Standalone(mut transport) => pipeline.query_async(&mut transport).await,
            Transport::Cluster(mut transport) => pipeline.query_async(&mut transport).await,
        }
        .map_err(backend)
    }

    pub(crate) async fn blocking_transport(
        &self,
    ) -> RedisResult<::redis::aio::MultiplexedConnection> {
        let Backend::Standalone { client, .. } = &self.client else {
            return Err(RedisError::UnsupportedClusterOperation(
                "queue blocking transport",
            ));
        };
        // The queue bounds blocking commands independently of the client response timeout.
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(None);
        tokio::time::timeout(
            Duration::from_secs(5),
            client.get_multiplexed_async_connection_with_config(&config),
        )
        .await
        .map_err(backend)?
        .map_err(backend)
    }

    async fn transport(&self) -> RedisResult<Transport> {
        self.transport
            .get_or_try_init(|| async {
                match &self.client {
                    Backend::Standalone { client, config } => client
                        .get_connection_manager_with_config(config.clone())
                        .await
                        .map(Transport::Standalone),
                    Backend::Cluster(cluster) => cluster
                        .client
                        .get_async_connection()
                        .await
                        .map(Transport::Cluster),
                }
            })
            .await
            .cloned()
            .map_err(backend)
    }

    pub(crate) fn standalone_location(&self) -> Option<(i64, &::redis::ConnectionAddr)> {
        let Backend::Standalone { client, .. } = &self.client else {
            return None;
        };
        let info = client.get_connection_info();
        Some((info.redis_settings().db(), info.addr()))
    }

    pub(crate) async fn server_id(&self) -> RedisResult<String> {
        let info: String = self.query(::redis::cmd("INFO").arg("server")).await?;
        info.lines()
            .find_map(|line| line.strip_prefix("run_id:").filter(|id| !id.is_empty()))
            .map(str::to_owned)
            .ok_or(RedisError::InvalidConfiguration)
    }

    /// Applies the connection prefix to a key used in a raw command.
    pub fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    pub async fn get(&self, key: &str) -> RedisResult<Option<Vec<u8>>> {
        self.query(::redis::cmd("GET").arg(self.key(key))).await
    }

    pub async fn set(&self, key: &str, value: &[u8]) -> RedisResult<()> {
        self.query(::redis::cmd("SET").arg(self.key(key)).arg(value))
            .await
    }

    pub async fn command(&self, method: &str, parameters: &[&[u8]]) -> RedisResult<Value> {
        let mut command = ::redis::cmd(method);
        for parameter in parameters {
            command.arg(*parameter);
        }
        self.query(&command).await
    }

    async fn subscription(&self, names: &[&str], pattern: bool) -> RedisResult<Subscription> {
        if names.is_empty() {
            return Err(crate::redis::RedisError::InvalidConfiguration);
        }
        match &self.client {
            Backend::Standalone { client, .. } => {
                let mut subscription =
                    tokio::time::timeout(Duration::from_secs(5), client.get_async_pubsub())
                        .await
                        .map_err(backend)?
                        .map_err(backend)?;
                if pattern {
                    subscription.psubscribe(names).await.map_err(backend)?;
                } else {
                    subscription.subscribe(names).await.map_err(backend)?;
                }
                Ok(Subscription::new(subscription.into_on_message()))
            }
            Backend::Cluster(cluster) => {
                let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                let mut transport = cluster
                    .client
                    .get_async_connection_with_config(ClusterConfig::new().set_push_sender(sender))
                    .await
                    .map_err(backend)?;
                if pattern {
                    transport.psubscribe(names).await.map_err(backend)?;
                } else {
                    transport.subscribe(names).await.map_err(backend)?;
                }
                Ok(Subscription::cluster(transport, receiver))
            }
        }
    }
}

impl Contract for Connection {
    fn command<'a>(
        &'a self,
        method: &'a str,
        parameters: &'a [&'a [u8]],
    ) -> RedisFuture<'a, RedisResult<Value>> {
        Box::pin(self.command(method, parameters))
    }

    fn subscribe<'a>(
        &'a self,
        channels: &'a [&'a str],
    ) -> RedisFuture<'a, RedisResult<Subscription>> {
        Box::pin(self.subscription(channels, false))
    }

    fn psubscribe<'a>(
        &'a self,
        patterns: &'a [&'a str],
    ) -> RedisFuture<'a, RedisResult<Subscription>> {
        Box::pin(self.subscription(patterns, true))
    }
}
