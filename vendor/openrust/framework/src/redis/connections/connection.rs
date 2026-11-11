use std::time::Duration;

use ::redis::{
    Client, FromRedisValue,
    aio::{ConnectionManager, ConnectionManagerConfig},
};
use tokio::sync::OnceCell;

use crate::contracts::redis::Connection as Contract;

use super::Subscription;
use crate::redis::{RedisFuture, RedisResult, Value, backend};

/// A named Redis command connection with a shared reconnecting transport.
pub struct Connection {
    client: Client,
    config: ConnectionManagerConfig,
    transport: OnceCell<ConnectionManager>,
    prefix: String,
}

impl Connection {
    pub(crate) fn new(client: Client, config: ConnectionManagerConfig, prefix: String) -> Self {
        Self {
            client,
            config,
            transport: OnceCell::new(),
            prefix,
        }
    }

    pub(crate) async fn query<T: FromRedisValue>(&self, command: &::redis::Cmd) -> RedisResult<T> {
        command
            .query_async(&mut self.transport().await?)
            .await
            .map_err(backend)
    }

    pub(crate) async fn pipeline(&self, pipeline: &::redis::Pipeline) -> RedisResult<()> {
        pipeline
            .query_async(&mut self.transport().await?)
            .await
            .map_err(backend)
    }

    async fn transport(&self) -> RedisResult<ConnectionManager> {
        self.transport
            .get_or_try_init(|| {
                self.client
                    .get_connection_manager_with_config(self.config.clone())
            })
            .await
            .cloned()
            .map_err(backend)
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
        let mut subscription =
            tokio::time::timeout(Duration::from_secs(5), self.client.get_async_pubsub())
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
