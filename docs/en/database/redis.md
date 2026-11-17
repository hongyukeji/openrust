# Redis

## Configuration

Define connections under `database.redis` in `config/database.rs`. The application provides a `default` connection and a `cache` connection. Configure the host, port and credentials through `REDIS_HOST`, `REDIS_PORT`, `REDIS_USERNAME` and `REDIS_PASSWORD`. Both connections use the logical database selected by `REDIS_DB`, which defaults to 0. Set `REDIS_CACHE_DB` to select a separate database for cache data. Clearing the Redis cache with `cache:clear redis` removes all keys in the cache database, including keys shared with other uses.

`REDIS_URL` can specify a Redis endpoint. A database in the URL takes precedence over the configured database. Use a URL without a database path to keep separate logical databases on the same server.

### Clusters {#clusters}

Add a named seed list under `database.redis.clusters` in `config/database.rs`. For a name defined both directly and under `clusters`, OpenRust uses the direct connection. `options.cluster` selects native Redis Cluster with the value `redis`. Native Cluster uses database 0 and discovers the remaining nodes from the seeds.

```json
{
  "clusters": {
    "sharded": [
      { "host": "127.0.0.1", "port": 7000 },
      { "url": "redis://127.0.0.1:7001/0" }
    ]
  }
}
```

Use `Redis::connection(Some("sharded"))` to select the cluster. Raw commands take Redis arguments. When a Redis command needs multiple keys in one slot, use a common hash tag such as `{reports}`.

```rust
use openrust::{redis::RedisResult, support::facades::Redis};

pub async fn cluster_report() -> RedisResult<Option<Vec<u8>>> {
    let connection = Redis::connection(Some("sharded"))?;
    connection.set("{reports}:latest", b"ready").await?;
    connection.get("{reports}:latest").await
}
```

When Redis returns `MOVED` or `ASK`, OpenRust follows the redirection for built-in single-key operations, including cache locks. The first seed’s `max_retries` limits these redirections. Set it to zero to return redirection errors directly. If the connection drops before Redis replies, OpenRust returns an error. The application decides whether to retry because Redis may already have applied the command. Later requests reconnect and discover the new primary after failover. Raw commands outside the supported single-key operations can return redirection errors. The queue worker’s dedicated blocking transport uses a standalone connection.

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

Set `REDIS_CACHE_CONNECTION` or `REDIS_CACHE_LOCK_CONNECTION` to a cluster name for cache values or individual locks. Cluster batch reads preserve input order across slots. Batch writes route each entry separately. `cache:clear` clears all primary nodes. Tagged cache currently uses standalone connections. During an explicit slot redirection, cache batch reads fall back to individual key reads.

Lock-only clearing supports a dedicated standalone database or a dedicated cluster. Redis flushes the entire lock backend. The cache and lock connections must point to different databases or clusters. When a cluster is involved, allow `CLUSTER INFO`, `CLUSTER NODES`, and `INFO CLUSTER` on a standalone peer so OpenRust can detect aliases before clearing.
