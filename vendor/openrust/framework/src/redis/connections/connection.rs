use std::{collections::HashSet, time::Duration};

use ::redis::{
    AsyncConnectionConfig, Client, ConnectionInfo, FromRedisValue,
    aio::{ConnectionManager, ConnectionManagerConfig},
    cluster::{ClusterClient, ClusterConfig},
};
use tokio::sync::OnceCell;

use crate::contracts::redis::Connection as Contract;

use super::{
    Subscription,
    cluster::{Cluster, ClusterTransport},
};
use crate::redis::{RedisError, RedisFuture, RedisResult, Value, backend};

/// A named Redis command connection with a shared reconnecting transport.
pub struct Connection {
    client: Backend,
    transport: OnceCell<Transport>,
    prefix: String,
    events: Option<(
        String,
        std::sync::Arc<crate::redis::redis_manager::CommandEvents>,
    )>,
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
    Cluster(ClusterTransport),
}

impl Connection {
    /// 通过原生 redis 客户端配置创建惰性连接，供自定义连接器复用。
    pub fn new(client: Client, config: ConnectionManagerConfig, prefix: String) -> Self {
        Self {
            client: Backend::Standalone { client, config },
            transport: OnceCell::new(),
            prefix,
            events: None,
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
            events: None,
        }
    }

    pub(crate) fn is_cluster(&self) -> bool {
        matches!(self.client, Backend::Cluster(_))
    }

    pub(crate) fn with_events(
        mut self,
        name: &str,
        events: std::sync::Arc<crate::redis::redis_manager::CommandEvents>,
    ) -> Self {
        self.events = Some((name.into(), events));
        self
    }

    pub(crate) async fn query<T: FromRedisValue>(&self, command: &::redis::Cmd) -> RedisResult<T> {
        self.query_command(command, false).await
    }

    // The caller has already ensured this command targets one Redis slot.
    pub(crate) async fn query_slot<T: FromRedisValue>(
        &self,
        command: &::redis::Cmd,
    ) -> RedisResult<T> {
        self.query_command(command, true).await
    }

    /// Give SCAN a routing key so Redis redirects it if the slot has moved.
    pub(crate) async fn scan_slot_keys(
        &self,
        key: &str,
        pattern: &str,
        cursor: u64,
        count: usize,
    ) -> RedisResult<(u64, Vec<Vec<u8>>)> {
        if !self.is_cluster() {
            return Err(RedisError::UnsupportedClusterOperation("slot-scoped SCAN"));
        }
        self.query_slot(
            ::redis::cmd("EVAL")
                .arg("return redis.call('SCAN', ARGV[1], 'MATCH', ARGV[2], 'COUNT', ARGV[3])")
                .arg(1)
                .arg(key)
                .arg(cursor)
                .arg(pattern)
                .arg(count),
        )
        .await
    }

    async fn query_command<T: FromRedisValue>(
        &self,
        command: &::redis::Cmd,
        known_single_target: bool,
    ) -> RedisResult<T> {
        let started = std::time::Instant::now();
        let result = match self.transport().await? {
            Transport::Standalone(mut transport) => command.query_async(&mut transport).await,
            Transport::Cluster(transport) => {
                let Backend::Cluster(cluster) = &self.client else {
                    unreachable!("cluster transport")
                };
                cluster.query(transport, command, known_single_target).await
            }
        }
        .map_err(backend)?;
        if let Some((name, events)) = &self.events
            && events.enabled.load(std::sync::atomic::Ordering::Acquire)
        {
            let dispatcher = events
                .dispatcher
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if let Some(dispatcher) = dispatcher {
                use crate::contracts::events::Dispatcher as _;
                let mut arguments = command.args_iter().map(|argument| match argument {
                    ::redis::Arg::Simple(bytes) => bytes.to_vec(),
                    _ => Vec::new(),
                });
                let command =
                    String::from_utf8_lossy(&arguments.next().unwrap_or_default()).into_owned();
                dispatcher
                    .dispatch(
                        std::sync::Arc::new(crate::redis::events::CommandExecuted {
                            command,
                            parameters: arguments.collect(),
                            time: started.elapsed(),
                            connection_name: name.clone(),
                        }),
                        &mut crate::events::EventContext::new(),
                    )
                    .await
                    .map_err(backend)?;
            }
        }
        Ok(result)
    }

    /// Executes a native command pipeline. Keys must already include their configured prefix.
    pub async fn pipeline(&self, pipeline: &::redis::Pipeline) -> RedisResult<()> {
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
                        .get_async_generic_connection()
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

    pub(crate) async fn cluster_members(&self) -> RedisResult<HashSet<String>> {
        if !self.is_cluster() {
            let info: String = self.query(::redis::cmd("INFO").arg("cluster")).await?;
            match info
                .lines()
                .find_map(|line| line.strip_prefix("cluster_enabled:"))
            {
                Some("0") => return Ok(HashSet::new()),
                Some("1") => (),
                _ => return Err(RedisError::InvalidConfiguration),
            }
        }
        let status: String = self.query(::redis::cmd("CLUSTER").arg("INFO")).await?;
        if !status.lines().any(|line| line == "cluster_state:ok") {
            return Err(RedisError::InvalidConfiguration);
        }
        let nodes: String = self.query(::redis::cmd("CLUSTER").arg("NODES")).await?;
        let mut members = HashSet::new();
        for line in nodes.lines() {
            let id = line
                .split_whitespace()
                .next()
                .ok_or(RedisError::InvalidConfiguration)?;
            if id.len() != 40 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(RedisError::InvalidConfiguration);
            }
            members.insert(id.to_owned());
        }
        if members.is_empty() {
            return Err(RedisError::InvalidConfiguration);
        }
        Ok(members)
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
