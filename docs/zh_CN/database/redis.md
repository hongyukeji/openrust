# Redis

## 配置

在 `config/database.rs` 的 `database.redis` 下定义连接。应用提供 `default` 和 `cache` 连接。通过 `REDIS_HOST`、`REDIS_PORT`、`REDIS_USERNAME` 和 `REDIS_PASSWORD` 配置地址及凭据。两个连接默认共用 `REDIS_DB` 指定的逻辑库，未设置时使用库 0。可用 `REDIS_CACHE_DB` 为缓存数据指定独立的逻辑库。执行 `cache:clear redis` 会清空缓存逻辑库的所有键，包括同库的其他用途键。

可通过 `REDIS_URL` 配置端点。URL 内的库号优先于配置库号。要让两个连接使用同一 Redis 实例的不同逻辑库，请使用省略库号的 URL。

### 集群 {#clusters}

在 `config/database.rs` 的 `database.redis.clusters` 中定义具名种子节点列表。同名直连和集群同时存在时，优先使用直连。`options.cluster` 使用 `redis` 选择原生 Redis Cluster。集群使用数据库 0，并通过种子节点发现其他节点。

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

通过 `Redis::connection(Some("sharded"))` 选择集群。原始命令接受 Redis 参数。需要多个键处于同一槽位时，使用共同的哈希标签，例如 `{reports}`。

```rust
use openrust::{redis::RedisResult, support::facades::Redis};

pub async fn cluster_report() -> RedisResult<Option<Vec<u8>>> {
    let connection = Redis::connection(Some("sharded"))?;
    connection.set("{reports}:latest", b"ready").await?;
    connection.get("{reports}:latest").await
}
```

集群的重试次数与延迟范围取首个种子节点的 `max_retries`、`backoff_base` 和 `backoff_cap`。原生客户端可能在连接错误后重试，结果未知的命令因此可能被重放。重试次数为零时也会关闭自动重定向重试。队列 Worker 的专用阻塞连接使用直连。

## 与 Redis 交互

在应用作用域内使用 `Redis` 门面。`Redis::get` 返回可选字节值，`Redis::set` 写入字节，两者都会应用 `REDIS_PREFIX`。用 `Redis::connection(Some("cache"))` 选择其他连接。

```rust
use openrust::{redis::RedisResult, support::facades::Redis};

pub async fn redis_report() -> RedisResult<Option<Vec<u8>>> {
    Redis::set("reports/latest", b"ready").await?;
    Redis::get("reports/latest").await
}
```

使用 `command` 执行原始 Redis 命令。需要给原始命令的键加前缀时，用 `connection.key("name")` 构造参数。Pub/Sub 使用独立连接。通过 Redis 连接契约调用 `subscribe` 或 `psubscribe`，再用 `next` 读取消息。

命令连接中断后，连接管理器会为后续请求重连。结果未确认的命令可能返回错误，应用应只重试可安全重复的操作。连接重试次数与延时上下限通过 `REDIS_MAX_RETRIES`、`REDIS_BACKOFF_BASE` 和 `REDIS_BACKOFF_CAP` 配置。

## Redis 缓存

设置 `CACHE_STORE=redis` 可选择 Redis 作为默认缓存。`REDIS_CACHE_CONNECTION` 指定数据连接，`REDIS_CACHE_LOCK_CONNECTION` 指定锁连接，默认分别为 `cache` 和 `default`。过期及锁操作见[缓存](../digging-deeper/cache.md)。

将 `REDIS_CACHE_CONNECTION` 或 `REDIS_CACHE_LOCK_CONNECTION` 设置为集群名称，可将缓存值或单个锁放在集群上。跨槽批量读保持输入顺序，批量写逐项路由。`cache:clear` 清空全部主节点。缓存标签和仅清锁目前使用直连。
