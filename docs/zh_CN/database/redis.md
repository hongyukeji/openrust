# Redis

## 配置

在 `config/database.rs` 的 `database.redis` 下定义连接。应用提供 `default` 和 `cache` 连接。通过 `REDIS_HOST`、`REDIS_PORT`、`REDIS_USERNAME` 和 `REDIS_PASSWORD` 配置地址及凭据。两个连接默认共用 `REDIS_DB` 指定的逻辑库，未设置时使用库 0。可用 `REDIS_CACHE_DB` 为缓存数据指定独立的逻辑库。执行 `cache:clear redis` 会清空缓存逻辑库的所有键，包括同库的其他用途键。

可通过 `REDIS_URL` 配置端点。URL 内的库号优先于配置库号。要让两个连接使用同一 Redis 实例的不同逻辑库，请使用省略库号的 URL。

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
