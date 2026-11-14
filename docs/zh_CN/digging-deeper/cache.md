# 缓存

## 配置

在 `config/cache.rs` 通过 `CACHE_STORE` 选择默认存储。 `array` 使用应用内存，`file` 使用本机进程可共享的目录，`database` 使用共享的 SQLite 或 MySQL 数据库。默认迁移创建 `cache` 和 `cache_locks` 表。启用数据库存储前，请对配置的数据库运行迁移。

```sh
./artisan migrate
```

## 缓存用法

用 `Cache::store(Some("file"))` 选择具名存储，也可以直接调用门面使用默认存储。缓存值支持 JSON 和字节。正数 `Ttl::Seconds` 设置有效期，`None` 表示永久保存。传给 `put` 的 TTL 为零或负数时会删除该项。`add` 原子插入缺失项，`touch` 延长或调整有效期，`pull` 原子读取并移除。

`cache:forget` 移除一个缓存项。`cache:clear` 清空所选数据存储，包括共用同一数据库表的所有前缀。锁使用独立命名空间。文件及数据库缓存使用 OpenRust 编码。

```sh
./artisan cache:forget reports/latest file
./artisan cache:clear file
```

## 缓存记忆化 {#cache-memoization}

用 `Cache::memo(None)` 选择默认仓储，或用 `Cache::memo(Some("redis"))` 选择命名仓储。首次查询访问后端，同一 HTTP 请求或队列任务内的重复查询复用命中及未命中结果。通过记忆化仓储修改条目会先清除对应的内存结果，再写入后端。普通仓储的写入在内存结果失效后或下一个请求、任务中可见。 在框架管理的生命周期之外，每次调用都会创建独立的记忆化仓储。从已结束的生命周期保留的仓储会直接读取后端。

```rust
use openrust::{
    cache::{CacheResult, Value},
    contracts::cache::Repository as _,
    support::facades::Cache,
};

pub async fn report() -> CacheResult<Option<Value>> {
    let cache = Cache::memo(Some("redis"))?;
    let _ = cache.get("reports/latest").await?;
    Cache::memo(Some("redis"))?.get("reports/latest").await
}
```

## 原子锁

用 `Cache::lock` 创建锁句柄。`get` 尝试一次，`block` 在限定时间内等待。持有者可释放或刷新有效锁。跨进程释放时，用持有者令牌调用 `Cache::restoreLock`。使用有限租期可在进程提前结束后恢复。

```rust
use std::time::Duration;

use openrust::{
    cache::{CacheResult, Ttl, Value},
    support::facades::Cache,
};

pub async fn cache_report() -> CacheResult<bool> {
    let mut lock = Cache::lock("report", Some(Duration::from_secs(10)))?;
    if !lock.get().await? {
        return Ok(false);
    }

    let written = Cache::put("reports/latest", &Value::from("ready"), Some(Ttl::Seconds(60))).await;
    let released = lock.release().await;
    let written = written?;
    released?;
    Ok(written)
}
```

## Redis 存储

设置 `CACHE_STORE=redis` 使用 [Redis 连接](../database/redis.md)存储缓存。原子取出使用 Redis `GETDEL`，该操作要求 Redis 6.2 或更高版本。

`cache:clear redis` 清空该缓存存储所用的整个逻辑库，包括该库内其他前缀的键。使用独立逻辑库的锁连接仍保留其锁。
