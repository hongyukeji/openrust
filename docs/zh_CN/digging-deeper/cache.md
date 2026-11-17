# 缓存

## 配置

在 `config/cache.rs` 通过 `CACHE_STORE` 选择默认存储。 `array` 使用应用内存，`file` 使用本机进程可共享的目录，`database` 使用共享的 SQLite 或 MySQL 数据库。

会话数据可以使用[会话缓存](../the-basics/session.md#session-cache)。`session` 仓库需要活跃的 HTTP 会话，不支持标签和缓存锁。

## 数据库缓存表 {#database-cache-tables}

对配置的数据库运行 `./artisan migrate`，应用自带的 `cache` 和 `cache_locks` 迁移。如果缺少该迁移，用 `make:cache-table` 或别名 `cache:table` 生成。执行生成的 Rust 迁移前需要重新构建应用。

```sh
./artisan make:cache-table
cargo build
./artisan migrate
```

## Memcached {#memcached}

设置 `CACHE_STORE=memcached` 启用 Memcached。默认地址为 `127.0.0.1:11211`，可用 `MEMCACHED_HOST` 和 `MEMCACHED_PORT` 修改。服务端启用 SASL PLAIN 时，同时设置 `MEMCACHED_USERNAME` 和 `MEMCACHED_PASSWORD`。 在 Unix 上，可将 `MEMCACHED_HOST` 设为套接字的绝对路径，并设置 `MEMCACHED_PORT=0`。

```dotenv
CACHE_STORE=memcached
MEMCACHED_HOST=127.0.0.1
MEMCACHED_PORT=11211
```

在 `config/cache.rs` 的 `cache.stores.memcached.servers` 配置多个节点。每个节点填写 `host`、`port` 和可选的正数 `weight`（默认 100）。共享缓存的应用进程应使用相同的节点及权重。`options` 支持 `connect_timeout_ms`、`timeout_ms`、`max_connections` 和 `max_item_bytes`。配置相同且使用同一 `MEMCACHED_PERSISTENT_ID` 的仓库在应用内复用连接池。

缓存值使用 OpenRust 编码。计数器不存在时返回 `CacheError::MissingCounter`，递减最低到零。支持按标签失效和校验所有者的锁。Memcached 内存紧张时可能淘汰条目，包括锁的所有者记录。`./artisan cache:clear memcached` 会清空所有已配置节点上的数据，覆盖所有缓存前缀。需要限制清理范围时可使用专用节点。

## DynamoDB {#dynamodb}

设置 `CACHE_STORE=dynamodb`，并创建以字符串字段 `key` 为分区键的 DynamoDB 表。默认表名是 `cache`。为数字字段 `expires_at` 启用 DynamoDB TTL，服务端即可回收过期行。缓存读取会检查过期时间，即使 DynamoDB 尚未删除该行。

用 `DYNAMODB_CACHE_TABLE` 指定表名，用 `AWS_DEFAULT_REGION` 指定区域。可以设置 `AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY` 和可选的 `AWS_SESSION_TOKEN`，也可以使用 AWS SDK 凭据链。DynamoDB Local 等自定义端点使用 `DYNAMODB_ENDPOINT`。

`config/cache.rs` 中的 `attributes` 支持自定义 `key`、`value` 和 `expiration` 字段名。分区键和 TTL 配置需要与表一致。OpenRust 使用 JSON、字节和 DynamoDB 原生数字存储缓存值。`forever` 和省略 TTL 的原子 `add` 将过期时间设为五年后。

该驱动支持校验所有者的锁。省略锁租期或设为零时使用 24 小时。缓存标签和整表清空不可用。可通过 `./artisan cache:forget <key> dynamodb` 删除单个条目。

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

### 读取缓存条目 {#retrieving-items-from-the-cache}

`getOr(key, default)` 接受默认值，`getOrElse(key, resolve)` 接受异步回退闭包。键缺失或缓存值为 JSON `null` 时使用默认值，命中时跳过回退。后端错误继续返回给调用方，回退计算的结果只返回给调用方。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_title() -> CacheResult<Value> {
    Cache::getOrElse("reports:title", || async { Ok(Value::from("Untitled")) }).await
}
```

### 类型化读取 {#typed-cache-values}

用 `string`、`integer`、`float`、`boolean` 和 `array` 校验读取结果。默认值传 `None` 或 `Some(value)`，键缺失或值为 null 时使用。惰性默认值用对应的 `OrElse` 方法。缓存值或默认值的类型不符合要求时返回 `CacheError::InvalidType`。字符串和布尔值按类型校验，数字方法接受有效的数字字符串，`array` 接受 JSON 数组和对象。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_limit() -> CacheResult<i64> {
    Cache::integer("reports:limit", Some(Value::from(10))).await
}
```

Redis 保留新写入的数字字符串及其原始写法。已有整数条目可继续按整数读取。增减数字字符串时会使用 Redis 脚本把条目转成整数，并保留到期时间。

### 批量操作 {#cache-batch-operations}

`getMultiple(keys, default)` 按输入顺序返回映射，可传入统一的默认值。`manyOr` 为每个键接收默认值，`manyOrElse` 按键计算惰性默认值。批量读取只调用一次存储，缺失键和 null 使用默认值。`set` 和 `setMultiple` 写入，`delete` 和 `deleteMultiple` 移除条目，`clear` 清空所选仓库。批量删除遇到缺失键会继续，并在任一键缺失时返回 `false`。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_state() -> CacheResult<Value> {
    let entries = Cache::manyOr(&[
        ("reports:title", Value::from("Untitled")),
        ("reports:limit", Value::from(10)),
    ]).await?;
    Ok(entries["reports:title"].clone())
}
```

### 读取并回填 {#retrieve-store}

`remember` 返回缓存值，未命中时执行计算并写入缓存。`rememberWithWarmth` 返回 `(value, warm)`，命中非空值时 `warm` 为 `true`。`rememberWithWarmthFor` 在未命中时根据计算结果设置 TTL。`rememberForever` 和别名 `sear` 将计算结果写入缓存且不设到期时间。

```rust
use openrust::{cache::{CacheResult, Ttl, Value}, support::facades::Cache};

pub async fn load_report() -> CacheResult<(Value, bool)> {
    Cache::rememberWithWarmth("reports:latest", Some(Ttl::Seconds(60)), async {
        Ok(Value::from("ready"))
    }).await
}
```

### 过期后后台刷新 {#swr}

`flexible(key, [fresh, expires], resolve)` 接收从同一次写入开始计时的两个秒数。在 `fresh` 之前直接读取缓存。在 `fresh` 和 `expires` 之间先返回旧值，再于成功的响应、命令或任务结束后刷新。缓存未命中或已到期时，先计算再返回。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn refresh_report() -> CacheResult<Value> {
    Cache::flexible("reports:summary", [5, 10], || async {
        Ok(Value::from("ready"))
    }).await
}
```

后台刷新使用缓存原子锁。如果其他调用者已经刷新同一代缓存，等待执行的刷新会跳过计算。刷新失败时，旧值可以继续使用到原定到期时间。`flexibleWith` 接受两个 `Ttl` 和 `FlexibleOptions`，可设置锁租期、所有者令牌或失败后仍刷新的 `always_defer`。在应用生命周期之外直接调用仓库时，可读取旧值直到到期。后台计算需要捕获自有值，以便在请求处理函数返回后执行。

### 取回并删除 {#retrieve-delete}

`pullOr(key, default)` 和 `pullOrElse(key, resolve)` 支持默认值。两者先执行仓库的原子 `pull`，条目缺失或值为 `null` 时再使用默认值。两个调用同时取回同一非空条目时，只有一个能获得该值。

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

## cache 助手 {#the-cache-helper}

`cache!()` 或 `openrust::support::cache()` 获取当前应用的缓存管理器。调用 `.store(None)?` 取得默认仓库。`cache!(key).await?` 返回 `Option<Value>`，`cache!(key, default).await?` 支持默认值。

写入形式接受至少一组键值和 `Option<Ttl>`。与固定 Laravel 的助手一样，它只写入第一项。需要写入全部条目时使用 `Cache::putMany`。

```rust
use openrust::{cache, cache::{CacheResult, Ttl, Value}};

pub async fn report() -> CacheResult<Value> {
    cache!({ "reports:latest" => Value::from("ready") }, Some(Ttl::Seconds(60))).await?;
    cache!("reports:latest", Value::from("pending")).await
}
```

## 仓库设置 {#cache-repository-settings}

`getName`、`getStore` 和 `getEventDispatcher` 分别读取已配置的仓库名称、底层存储和事件分发器。具名仓库关闭缓存事件后仍可读取名称。创建标签仓库时，它会继承来源仓库的名称、事件分发器和默认时长。

`putWithDefault` 使用 `getDefaultCacheTime` 返回的时长，初始值为 3600 秒。显式调用 `put(key, value, None)` 会永久保存。用 `setDefaultCacheTime` 修改自有仓库的默认时长，`setStore` 和 `setEventDispatcher` 也用于自有仓库。克隆后的仓库共享底层存储，每份副本可各自替换设置。

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn cache_report() -> CacheResult<bool> {
    let mut repository = Cache::store(Some("redis"))?.as_ref().clone();
    repository.setDefaultCacheTime(Some(60));
    let reports = repository.tags(&["reports"])?;
    reports.putWithDefault("latest", &Value::from("ready")).await
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

在 Redis Cluster 上，框架根据连接前缀和存储前缀为标签命名空间选择一个哈希槽，使值与标签索引能够通过 Lua 原子更新。过期清理分批扫描当前存储的标签索引。如果标签清空中断并留下待回收的旧代际，过期清理会在该代际的索引清空时移除对应记录。普通缓存键继续按集群规则分片。单槽成为瓶颈时，可以用不同存储或前缀分散标签缓存负载。

## 原子锁

用 `Cache::lock` 创建锁句柄。`get` 尝试一次，`block` 在限定时间内等待。持有者可释放或刷新有效锁。跨进程释放时，用持有者令牌调用 `Cache::restoreLock`。使用有限租期可在进程提前结束后恢复。

### 锁状态与重试间隔 {#lock-status}

`isLocked().await?` 查询后端是否持有锁。`isOwnedBy(token).await?` 比对指定令牌，`isOwnedByCurrentProcess().await?` 比对当前句柄的令牌。`isOwnedBy(None).await?` 检查是否没有所有者。状态查询不会获取锁或续期。`null` 存储始终报告未锁定，所有者查询使用当前句柄的令牌。

`betweenBlockedAttemptsSleepFor(milliseconds)` 设置返回句柄的 `block` 和 `blockWith` 重试间隔，默认 250 毫秒。间隔为 0 时，每次重试前让出 Tokio 执行权。

```rust
use std::time::Duration;

use openrust::{cache::CacheResult, support::facades::Cache};

pub async fn ready_for_report() -> CacheResult<bool> {
    let mut lock = Cache::lock("report", Some(Duration::from_secs(10)))?
        .betweenBlockedAttemptsSleepFor(25)?;
    lock.block(Duration::from_secs(2)).await?;
    let owned = lock.isOwnedByCurrentProcess().await;
    lock.release().await?;
    owned
}
```

### 持锁执行回调 {#lock-callbacks}

用 `getWith` 尝试一次，或用 `blockWith` 等待。两者接管锁句柄，在回调返回时释放锁，包括回调出错的情况。锁被占用时，`getWith` 返回 `None`。等待超时则由 `blockWith` 返回 `CacheError::LockTimeout`。获取失败时均不会调用回调。

```rust
use std::time::Duration;

use openrust::{
    cache::{CacheResult, Ttl, Value},
    support::facades::Cache,
};

pub async fn cache_report() -> CacheResult<Option<bool>> {
    Cache::lock("report", Some(Duration::from_secs(10)))?
        .getWith(|| async {
            Cache::put("reports/latest", &Value::from("ready"), Some(Ttl::Seconds(60))).await
        })
        .await
}
```

Future 被取消或回调 panic 时，Tokio 会调度清理。运行时退出导致清理无法执行时，有限租期允许锁到期。

### 并发限制 {#concurrency-limiting}

`Cache::withoutOverlapping(key, callback).await?` 对同一键一次只执行一个回调，最多等待 10 秒。`withoutOverlappingWith(key, callback, lock_for, wait_for, owner)` 以秒指定租期和等待时间，并接受可选的所有者令牌。租期为零时使用驱动默认值。无法获得锁时返回 `CacheError::LockTimeout`。

使用 `Cache::funnel` 允许多个回调并行执行。各应用实例须使用同一个共享锁存储、缓存前缀、限制器名称和并发上限。租期应覆盖回调的执行时间，到期后其他调用者可以取得该名额。`null` 仓库允许所有调用通过。

```rust
use openrust::{cache::CacheResult, support::facades::Cache};

pub async fn generate_report() -> CacheResult<&'static str> {
    Cache::funnel("reports:generate:")?
        .limit(3)
        .releaseAfter(60)
        .block(10)
        .thenWith(|| async { Ok("generated") }, |_| async { Ok("busy") })
        .await
}
```

用 `limit` 设置并发名额。租期默认 60 秒，等待默认 3 秒，`sleep(milliseconds)` 默认 250 毫秒。`block(0)` 只扫描一次名额。`then(callback)` 返回 `CacheError::LimiterTimeout`，`thenWith(callback, fallback)` 可在回调中处理该错误。可用 `Cache::store(Some("redis"))?.funnel(name)?` 选取具名仓库。缺少原子锁能力的仓库返回 `CacheError::LocksUnsupported`。

### 清除缓存锁 {#clearing-cache-locks}

使用 `Cache::flushLocks().await?` 清除默认存储的锁，或用 `Cache::store(Some("database"))?.flushLocks().await?` 指定存储。Artisan 同样可以指定存储名。

```sh
./artisan cache:clear database --locks
./artisan cache:clear redis --tags=reports,daily
```

清锁命令会清除所选锁存储中的所有锁，包括同一后端中其他前缀的锁。数组和文件存储分别保存锁与普通条目。数据库需要独立锁表。Redis 会清空整个专用锁库。同一 Redis 实例应使用不同的逻辑库编号。不同入口配置相同库编号时，OpenRust 会先核对 Redis 实例标识，连接需要允许 `INFO SERVER`。

仅清锁支持独立 Redis 逻辑库或独立 Redis Cluster。Redis 会清空整个锁后端。缓存连接与锁连接须指向不同的逻辑库或集群。涉及集群时，需要允许 `CLUSTER INFO`、`CLUSTER NODES`，直连的一侧还需允许 `INFO CLUSTER`，以便框架在清理前识别指向同一集群的别名。 [Redis 集群配置](../database/redis.md)

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
