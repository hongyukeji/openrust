# 缓存

## 配置

在 `config/cache.rs` 通过 `CACHE_STORE` 选择默认存储。 `array` 使用应用内存，`file` 使用本机进程可共享的目录，`database` 使用共享的 SQLite 或 MySQL 数据库。默认迁移创建 `cache` 和 `cache_locks` 表。启用数据库存储前，请对配置的数据库运行迁移。

```sh
./artisan migrate
```

会话数据可以使用[会话缓存](../the-basics/session.md#session-cache)。`session` 仓库需要活跃的 HTTP 会话，不支持标签和缓存锁。

## 磁盘缓存 {#storage-cache}

通过 `CACHE_STORE=storage` 将缓存记录写入已配置的文件系统磁盘。`CACHE_STORAGE_DISK` 指定磁盘，省略时使用应用默认磁盘。`CACHE_STORAGE_PATH` 是相对于该磁盘的缓存目录。

```dotenv
CACHE_STORE=storage
CACHE_STORAGE_DISK=local
CACHE_STORAGE_PATH=framework/cache/items
```

本地磁盘与 S3 共享同一缓存接口。过期或无法读取的记录视为未命中，后续写入可覆盖过期文件，清理也可删除它们。与 Laravel 的磁盘驱动相同，读改写操作没有跨进程原子性。该驱动不提供锁和标签。

`./artisan cache:clear storage` 会清理该缓存目录中所有前缀的文件。缓存路径为空时会清理所选磁盘中的文件。磁盘还保存普通文件时，请指定专用缓存目录。

## 空缓存存储 {#null-store}

设置 `CACHE_STORE=null` 可选择 `null` 存储，单次操作可用 `Cache::store(Some("null"))` 指定。应用没有配置默认存储时，缓存管理器也会使用 `null`。读取总是未命中，写入与修改 TTL 返回 `false`，计数返回 `CacheError::MissingCounter`。删除、清理条目和清锁仍返回成功。两个同名锁句柄都能获取锁，`null` 存储没有互斥能力。

```sh
./artisan cache:forget reports/latest null
./artisan cache:clear null --tags=reports
./artisan cache:clear null --locks
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

## 缓存标签 {#cache-tags}

使用 `Cache::tags` 操作默认存储，或通过 `Cache::store(Some("array"))?.tags(...)` 指定存储。`array`、`redis`、`failover` 和 `null` 接受标签。当前直接选择其他存储会返回标签不受支持的错误。

```rust
use openrust::{
    cache::{CacheError, Ttl, Value},
    contracts::cache::Repository as _,
    support::facades::Cache,
};

async fn store_report() -> Result<bool, CacheError> {
    Cache::tags(&["reports", "daily"])?
        .put("latest", &Value::from("ready"), Some(Ttl::Seconds(60)))
        .await
}
```

读取时传入完全相同的标签顺序，包括重复项。调用 `Cache::tags(&["daily"])?.flush().await?` 可使所有带 `daily` 标签的条目失效。普通键和只含无关标签的条目会保留。标签通过切换代际失效。`array` 和 `failover` 的旧条目无法再按标签读到，按原 TTL 到期。直接选择 Redis 存储时还会分批删除旧代际的实际键。Redis 在同一脚本内写入条目和更新索引，清理旧代际期间仍可写入新代际。

运行 `./artisan cache:prune-stale-tags redis` 可清理已过期或已删除值的索引引用，仍有效的值保留。可显式指定存储名，也可省略以使用默认存储。

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

### 清除缓存锁 {#clearing-cache-locks}

使用 `Cache::flushLocks().await?` 清除默认存储的锁，或用 `Cache::store(Some("database"))?.flushLocks().await?` 指定存储。Artisan 同样可以指定存储名。

```sh
./artisan cache:clear database --locks
./artisan cache:clear redis --tags=reports,daily
```

清锁命令会清除所选锁存储中的所有锁，包括同一后端中其他前缀的锁。数组和文件存储分别保存锁与普通条目。数据库需要独立锁表。Redis 会清空整个专用锁库。同一 Redis 实例应使用不同的逻辑库编号。不同入口配置相同库编号时，OpenRust 会先核对 Redis 实例标识，连接需要允许 `INFO SERVER`。

`--tags` 清理匹配的标签条目。非空 `--tags` 与 `--locks` 同时使用会失败。

## Redis 存储

设置 `CACHE_STORE=redis` 使用 [Redis 连接](../database/redis.md)存储缓存。原子取出使用 Redis `GETDEL`，该操作要求 Redis 6.2 或更高版本。

`cache:clear redis` 清空该缓存存储所用的整个逻辑库，包括该库内其他前缀的键。使用独立逻辑库的锁连接仍保留其锁。

## 缓存故障切换 {#cache-failover}

设置 `CACHE_STORE=failover` 可将 `failover` 设为默认缓存存储。`config/cache.rs` 中该驱动的 `stores` 列表决定尝试顺序。应用默认先尝试 `database`，再尝试 `array`。需要优先使用 Redis 时，可配置为 `["redis", "database", "array"]`。

每次操作都从第一个存储开始。存储报错时才尝试下一个，缓存未命中或成功返回 `false` 时直接结束。主存储恢复后会再次优先读取它，故障期间写入备用存储的条目此时可能读不到。所有存储均失败时，操作返回最后一个错误。

在应用事件分发器中监听 `openrust::cache::events::CacheFailedOver`，可获知新出现的存储故障。事件包含 `storeName` 和 `exception` 错误消息。同一存储连续报错只产生一次事件，经过一次该存储未报错的操作后，再次报错才会重新触发。锁句柄由第一个能创建它的存储提供，后续获取、续期和释放都使用该存储。

```rust
use openrust::{
    cache::{CacheManager, CacheResult, Value},
    contracts::cache::Repository as _,
};

pub async fn cache_status(manager: &CacheManager) -> CacheResult<Option<Value>> {
    manager.store(Some("failover"))?.get("status").await
}
```

## 添加自定义缓存驱动 {#adding-custom-cache-drivers}

为后端实现 `openrust::contracts::cache::Store`，在服务提供者的 `boot` 阶段注册驱动，并确保注册早于首次解析存储。下面用独立内存存储演示注册方式。

```rust
use std::sync::Arc;

use openrust::{cache::ArrayStore, support::facades::Cache};

Cache::extend("project", |manager, config| {
    manager.repository(Arc::new(ArrayStore::new()), config)
})?;
```

在 `config/cache.rs` 中将具名存储的 `driver` 设为 `project`。工厂收到缓存管理器和完整存储配置。用 `manager.getContainer()` 获取应用服务，用 `manager.repository(..., config)` 保留存储名和事件设置。存储解析是同步的，回调负责构造后端，异步 I/O 放在具体缓存操作中。

具名仓库在首次访问时创建，后续继续复用。替换工厂后，可调用 `Cache::forgetDriver(Some("reports"))?` 或 `Cache::purge(Some("reports"))?`，下一次解析就使用新工厂。`Cache::build(config)?` 按给定配置即时创建独立仓库。

## 缓存事件 {#cache-events}

通过应用事件分发器监听 `CacheHit::NAME`、`KeyWritten::NAME` 和 `CacheLocksFlushed::NAME` 等事件。`openrust::cache::events` 中的事件携带所选存储名。读写事件提供调用方传入的键和适用的有序标签。批量读取先派发 `RetrievingManyKeys`，再逐个派发命中或未命中事件。

在 `config/cache.rs` 的具体存储配置中设置 `events: false` 可关闭该仓库的事件。故障切换仓库默认关闭，底层每个存储仍按自身配置派发。记忆化仓库在访问底层时派发事件，命中记忆化快照的重复读取没有额外事件。

同步监听器失败时，操作返回错误。写入前的监听器失败会阻止写入，写入后的监听器失败会保留已写入的值。原子 `pull` 的读取和删除前置事件都在存储操作前发出。

测试缓存命中时，先写入 `reports/latest`，再通过已启动的 `TestApp` 读取并断言：

```rust
use openrust::{cache::events::CacheHit, contracts::cache::Repository as _};

let events = app.fakeEvents(&[CacheHit::NAME]).await?;
let _ = cache.get("reports/latest").await?;
events.assertDispatched(CacheHit::NAME);
```
