use crate::redis::{RedisFuture, RedisResult, Value, connections::Subscription};

/// Executes Redis commands and starts isolated subscriptions.
pub trait Connection: Send + Sync {
    fn command<'a>(
        &'a self,
        method: &'a str,
        parameters: &'a [&'a [u8]],
    ) -> RedisFuture<'a, RedisResult<Value>>;

    fn subscribe<'a>(
        &'a self,
        channels: &'a [&'a str],
    ) -> RedisFuture<'a, RedisResult<Subscription>>;

    fn psubscribe<'a>(
        &'a self,
        patterns: &'a [&'a str],
    ) -> RedisFuture<'a, RedisResult<Subscription>>;
}
