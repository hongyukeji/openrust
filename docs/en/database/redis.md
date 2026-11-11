# Redis

## Configuration

Define connections under `database.redis` in `config/database.rs`. The application provides a `default` connection and a `cache` connection. Configure the host, port and credentials through `REDIS_HOST`, `REDIS_PORT`, `REDIS_USERNAME` and `REDIS_PASSWORD`. The default logical databases are 0 and 1, selected by `REDIS_DB` and `REDIS_CACHE_DB`.

`REDIS_URL` can specify a Redis endpoint. A database in the URL takes precedence over the configured database. Use a URL without a database path to keep separate logical databases on the same server.

## Interacting With Redis

Use the `Redis` facade inside the application scope. `Redis::get` returns optional bytes, and `Redis::set` writes bytes. These methods apply `REDIS_PREFIX`. Select another connection with `Redis::connection(Some("cache"))`.

```rust
use openrust::{redis::RedisResult, support::facades::Redis};

pub async fn redis_report() -> RedisResult<Option<Vec<u8>>> {
    Redis::set("reports/latest", b"ready").await?;
    Redis::get("reports/latest").await
}
```

Use `command` for raw Redis commands. To prefix a raw key argument, build it with `connection.key("name")`. Pub/Sub uses a dedicated connection. Call `subscribe` or `psubscribe` through the Redis connection contract, then read messages with `next`.

A lost command connection reconnects for subsequent requests. A command with an uncertain outcome can return an error. Only retry operations that your application can safely repeat. The reconnect budget and delay bounds come from `REDIS_MAX_RETRIES`, `REDIS_BACKOFF_BASE` and `REDIS_BACKOFF_CAP`.

## Redis Cache

Set `CACHE_STORE=redis` to select Redis as the default cache. The cache uses `REDIS_CACHE_CONNECTION` for data and `REDIS_CACHE_LOCK_CONNECTION` for locks, with defaults of `cache` and `default`. See [Cache](../digging-deeper/cache.md) for expiry and lock operations.
