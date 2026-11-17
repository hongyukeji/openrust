# Cache

## Configuration

The default store is selected by `CACHE_STORE` in `config/cache.rs`. Use `array` for application-local memory, `file` for a directory shared by local processes, or `database` for a shared SQLite or MySQL store.

Session data can use the [session cache](../the-basics/session.md#session-cache). The `session` store requires an active HTTP session and provides neither tags nor cache locks.

## Database cache tables {#database-cache-tables}

Run `./artisan migrate` against the configured database to apply the bundled migration for `cache` and `cache_locks`. If the migration is missing, generate it with `make:cache-table` or its `cache:table` alias. Rebuild the application before running the generated Rust migration.

```sh
./artisan make:cache-table
cargo build
./artisan migrate
```

## Memcached {#memcached}

Set `CACHE_STORE=memcached` to use Memcached. The default endpoint is `127.0.0.1:11211`. Set `MEMCACHED_HOST` and `MEMCACHED_PORT` to change it. Set both `MEMCACHED_USERNAME` and `MEMCACHED_PASSWORD` for a server configured for SASL PLAIN. On Unix, set `MEMCACHED_HOST` to the absolute socket path and `MEMCACHED_PORT=0` to use a Unix socket.

```dotenv
CACHE_STORE=memcached
MEMCACHED_HOST=127.0.0.1
MEMCACHED_PORT=11211
```

Set `cache.stores.memcached.servers` in `config/cache.rs` to use multiple servers. Each server takes `host`, `port`, and an optional positive `weight` (default 100). All application processes sharing the cache must use the same server list and weights. The `options` map accepts `connect_timeout_ms`, `timeout_ms`, `max_connections`, and `max_item_bytes`. Matching stores with the same `MEMCACHED_PERSISTENT_ID` share the connection pool within an application.

Values use OpenRust encoding. A missing counter returns `CacheError::MissingCounter`, and decrement stops at zero. Tag invalidation and owner-checked locks are supported. Memcached may evict entries, including lock owners, under memory pressure. `./artisan cache:clear memcached` flushes every configured server across all cache prefixes. Use dedicated servers if this scope is too broad.

## DynamoDB {#dynamodb}

Set `CACHE_STORE=dynamodb` and create a DynamoDB table with a string partition key named `key`. The default table is `cache`. Enable DynamoDB TTL on the numeric `expires_at` attribute so the service can reclaim expired rows. Cache reads check expiry even while a row awaits DynamoDB cleanup.

Set `DYNAMODB_CACHE_TABLE` to select the table and `AWS_DEFAULT_REGION` to select the region. Supply `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and optional `AWS_SESSION_TOKEN`, or use the AWS SDK credential chain. Use `DYNAMODB_ENDPOINT` for a custom endpoint such as DynamoDB Local.

The `attributes` configuration in `config/cache.rs` accepts `key`, `value`, and `expiration` names. Match the partition key and TTL configuration on the table. OpenRust stores JSON, bytes, and native DynamoDB numbers. The five-year expiry for `forever` also applies to an atomic `add` without a TTL.

The driver supports owner-checked locks. Locks without a positive lease use 24 hours. Cache tags and whole-table flush are unavailable. Use `./artisan cache:forget <key> dynamodb` to remove a single entry.

## Storage Cache {#storage-cache}

Set `CACHE_STORE=storage` to store cache records on a configured filesystem disk. `CACHE_STORAGE_DISK` selects the disk and uses the application's default disk when omitted. `CACHE_STORAGE_PATH` selects the cache directory relative to that disk.

```dotenv
CACHE_STORE=storage
CACHE_STORAGE_DISK=local
CACHE_STORAGE_PATH=framework/cache/items
```

Local and S3 disks use the same cache API. Expired or unreadable records are cache misses. Expired files can be overwritten by a later write or removed by a clear. Like Laravel's storage driver, read-modify-write operations do not provide cross-process atomicity. This driver has no cache locks or tags.

`./artisan cache:clear storage` clears all cache-directory files across cache prefixes. An empty cache path clears files across the selected disk. Use a dedicated cache directory on a disk shared with ordinary files.

## Null Store {#null-store}

Set `CACHE_STORE=null` to select the `null` store, or use `Cache::store(Some("null"))` for a single operation. The cache manager also uses `null` when the application has no configured default store. Reads always miss. Writes and TTL changes return `false`, and counter operations return `CacheError::MissingCounter`. Forgetting keys, clearing values and clearing locks succeed without keeping state. Both handles for the same lock name can acquire it, so the `null` store provides no mutual exclusion.

```sh
./artisan cache:forget reports/latest null
./artisan cache:clear null --tags=reports
./artisan cache:clear null --locks
```

## Cache Usage

Use `Cache::store(Some("file"))` for a named store or call the facade directly for the default store. Entries hold JSON values or bytes. A positive `Ttl::Seconds` sets a lifetime, and `None` stores the value indefinitely. A zero or negative TTL deletes an entry when passed to `put`. `add` atomically inserts a missing entry. `touch` updates its lifetime. `pull` atomically retrieves and removes it.

`cache:forget` removes one entry. `cache:clear` clears the selected data store, including every prefix sharing its database table. The lock store has a separate namespace. Cache files and database entries use OpenRust encoding.

```sh
./artisan cache:forget reports/latest file
./artisan cache:clear file
```

### Retrieving items from the cache {#retrieving-items-from-the-cache}

Use `getOr(key, default)` for a value or `getOrElse(key, resolve)` for an asynchronous fallback. Missing entries and cached JSON `null` use the default. A hit leaves the fallback untouched. Backend errors still propagate, and fallback results are only returned to the caller.

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_title() -> CacheResult<Value> {
    Cache::getOrElse("reports:title", || async { Ok(Value::from("Untitled")) }).await
}
```

### Typed cache reads {#typed-cache-values}

Use `string`, `integer`, `float`, `boolean`, or `array` to validate the value after a read. Pass `None` for no default or `Some(value)` to cover a missing or null entry. Use the matching `OrElse` method for a lazy default. An incompatible value or default returns `CacheError::InvalidType`. Strings and booleans are checked by type, numeric reads accept validated numeric strings, and `array` accepts JSON arrays and objects.

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn report_limit() -> CacheResult<i64> {
    Cache::integer("reports:limit", Some(Value::from(10))).await
}
```

Redis preserves the spelling and type of newly written numeric strings. Existing integer entries remain readable as integers. Incrementing or decrementing a numeric string converts it to an integer and preserves its lifetime. This conversion uses a Redis script.

### Batch operations {#cache-batch-operations}

`getMultiple(keys, default)` returns a map in input order with a shared optional default. `manyOr` accepts a default for each key, and `manyOrElse` resolves defaults lazily by key. The reads query the store once, and a missing or null value uses the default. Use `set` and `setMultiple` to write, `delete` and `deleteMultiple` to remove entries, and `clear` to flush the selected store. A batch delete attempts the remaining keys after a missing entry and returns `false` if any key was absent.

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

### Retrieve and store {#retrieve-store}

`remember` returns a cached value or resolves and stores it on a miss. `rememberWithWarmth` returns `(value, warm)`. `warm` is `true` for a non-null cache hit. `rememberWithWarmthFor` computes the TTL from the resolved value on a miss. `rememberForever` and its alias `sear` store the result without an expiry.

```rust
use openrust::{cache::{CacheResult, Ttl, Value}, support::facades::Cache};

pub async fn load_report() -> CacheResult<(Value, bool)> {
    Cache::rememberWithWarmth("reports:latest", Some(Ttl::Seconds(60)), async {
        Ok(Value::from("ready"))
    }).await
}
```

### Stale while revalidate {#swr}

`flexible(key, [fresh, expires], resolve)` takes two durations in seconds from the same write. Before `fresh`, it reads the cached value. Between `fresh` and `expires`, it returns the stale value and defers refresh until after a successful response, command, or job. A cold or expired value is resolved before returning.

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn refresh_report() -> CacheResult<Value> {
    Cache::flexible("reports:summary", [5, 10], || async {
        Ok(Value::from("ready"))
    }).await
}
```

Deferred refresh uses an atomic cache lock. A queued refresh skips its resolver if another caller has refreshed the same generation. A failed refresh leaves the old value available until its original expiry. `flexibleWith` accepts two `Ttl` values and `FlexibleOptions` for a custom lock lease, owner token, or `always_defer` after failure. A repository call outside an application lifecycle can read stale data until expiry. Deferred resolvers capture owned values so they can outlive the request handler.

### Retrieve and delete {#retrieve-delete}

Use `pullOr(key, default)` or `pullOrElse(key, resolve)` with a default. Both use the store's atomic `pull` before resolving a missing or null value. If two callers pull the same non-null entry, only one receives it.

## Cache Memoization {#cache-memoization}

Use `Cache::memo(None)` for the default store or `Cache::memo(Some("redis"))` for a named store. The first lookup reads the backend. Repeated lookups reuse hits and misses in the current HTTP request or queue job. Changes made through the memoized repository invalidate the affected keys before forwarding to the backend. Writes made through the plain repository become visible after invalidation or in the next request or job. Outside a managed lifecycle, each call creates a separate memoized repository. A repository retained from a completed lifecycle reads the backend directly.

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

## The cache helper {#the-cache-helper}

`cache!()` or `openrust::support::cache()` resolves the current application's manager. Use `.store(None)?` on it to get the default repository. `cache!(key).await?` reads a value as `Option<Value>`, and `cache!(key, default).await?` supplies a default.

The write form accepts a nonempty set of key/value pairs and an `Option<Ttl>`. Like the fixed Laravel helper, it writes the first entry. Use `Cache::putMany` to write all entries.

```rust
use openrust::{cache, cache::{CacheResult, Ttl, Value}};

pub async fn report() -> CacheResult<Value> {
    cache!({ "reports:latest" => Value::from("ready") }, Some(Ttl::Seconds(60))).await?;
    cache!("reports:latest", Value::from("pending")).await
}
```

## Repository settings {#cache-repository-settings}

`getName`, `getStore`, and `getEventDispatcher` expose the configured store name, backend, and dispatcher. A named repository keeps its name when cache events are disabled. A tagged repository inherits the name, dispatcher, and default lifetime of its parent at creation.

`putWithDefault` uses `getDefaultCacheTime`, which starts at 3600 seconds. An explicit `put(key, value, None)` stores the value indefinitely. Use `setDefaultCacheTime` on an owned repository to change the default. `setStore` and `setEventDispatcher` also configure an owned repository. Cloning shares the backend while allowing each copy to replace its own settings.

```rust
use openrust::{cache::{CacheResult, Value}, support::facades::Cache};

pub async fn cache_report() -> CacheResult<bool> {
    let mut repository = Cache::store(Some("redis"))?.as_ref().clone();
    repository.setDefaultCacheTime(Some(60));
    let reports = repository.tags(&["reports"])?;
    reports.putWithDefault("latest", &Value::from("ready")).await
}
```

## Cache Tags {#cache-tags}

Use `Cache::tags` for the default store or `Cache::store(Some("array"))?.tags(...)` for a named store. The `array`, `redis`, `failover`, and `null` stores accept tags. Other directly selected stores currently return a tagging error.

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

Read the item with the same ordered list, including any repeated names. To invalidate any item tagged `daily`, call `Cache::tags(&["daily"])?.flush().await?`. The flush preserves plain entries and entries carrying only unrelated tags. Tagged flushes change generations. With `array` and `failover`, previous entries become unreachable and expire according to their original TTL. A directly selected Redis store also deletes the retired keys in batches. Tagged writes and index updates use a single Redis script, and the active generation can accept new writes while retired keys are removed.

To remove stale references from selected Redis tags, call `Cache::tags(&["daily"])?.flushStale().await?`. It scans the current generation of each selected tag and preserves live values and other tag indexes. An empty list leaves the store untouched.

Run `./artisan cache:prune-stale-tags redis` to remove sorted-set references to expired or deleted values. This command preserves live values. Pass a store name, or omit it to use the default store.

On Redis Cluster, store-wide pruning also discovers tag indexes on the primary that owns the store's tag slot. If Redis evicts the index registry, pruning rediscovers surviving indexes, removes stale references, and keeps live values.

During Redis Cluster slot migration, Redis may return `TRYAGAIN` when a cleanup batch references missing keys or its keys are split between nodes. When it does, `flushStale()` returns an error and `cache:prune-stale-tags` exits with status 1. After slot migration completes, run the same cleanup again. Batches that already completed stay applied, and retrying skips references that were already removed.

If a Redis Cluster tag flush is interrupted, run `./artisan cache:clear redis --tags=artists` again to finish removing the retired values. A retry drains valid retired generations and discards malformed generation identifiers. If a Redis key has an unexpected type, the command exits with status 1. Restore the expected key type before retrying.

On Redis Cluster, a tagged cache namespace uses one hash slot derived from the connection and store prefixes. This keeps each value and its tag indexes in the same slot for atomic Lua updates. When `forget` or `pull` removes the last member of a tag index, the same update removes its registry entry. Tag flushes and pruning process indexes in batches. Both remove malformed references and references outside the store’s tagged namespace without modifying the keys they name. Pruning also unregisters malformed index entries. If an interrupted tag flush leaves an old generation pending, pruning clears that record once the generation's index is empty. Untagged keys continue to use normal cluster routing. Distribute tagged workloads across stores or prefixes when one slot becomes a bottleneck.

## Atomic Locks

Use `Cache::lock` to create a lock handle. `get` tries once and `block` waits for a bounded interval. An owner can release or refresh a live lock. To release it from another process, pass the owner token to `Cache::restoreLock`. Use a finite lease to recover if the process ends before release.

### Lock status and retry interval {#lock-status}

`isLocked().await?` checks whether the backend holds the lock. `isOwnedBy(token).await?` compares an owner token, while `isOwnedByCurrentProcess().await?` compares the token of this handle. `isOwnedBy(None).await?` checks for no owner. Status checks do not acquire or renew a lock. The `null` store always reports unlocked and uses the handle token for ownership checks.

`betweenBlockedAttemptsSleepFor(milliseconds)` sets the interval for `block` and `blockWith` on the returned handle. The default is 250 ms. A zero interval yields to other Tokio tasks between attempts.

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

### Run a callback while holding a lock {#lock-callbacks}

Use `getWith` to try once or `blockWith` to wait. Both take ownership of the handle and release its lock after the callback returns, including an error result. `getWith` returns `None` on contention. `blockWith` returns `CacheError::LockTimeout` on timeout. Neither invokes the callback when acquisition fails.

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

Tokio schedules cleanup if the future is cancelled or the callback panics. A finite lease still lets the lock expire when runtime shutdown prevents cleanup.

### Concurrency limiting {#concurrency-limiting}

`Cache::withoutOverlapping(key, callback).await?` runs one callback at a time for that key and waits up to 10 seconds. `withoutOverlappingWith(key, callback, lock_for, wait_for, owner)` takes the lease and wait in seconds and an optional owner token. A zero lease uses the store's default. An unavailable lock returns `CacheError::LockTimeout`.

Use `Cache::funnel` to allow several concurrent callbacks. All application instances must use the same shared lock store, cache prefix, limiter name, and limit. Set the lease long enough for a callback to finish. Once it expires, another caller can take the slot. A `null` store allows every caller through.

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

Set the slot count with `limit`. The default lease and wait are 60 and 3 seconds. `sleep(milliseconds)` defaults to 250 ms. Set `block(0)` to scan the slots once. Use `then(callback)` to receive `CacheError::LimiterTimeout`, or `thenWith(callback, fallback)` to handle that error in a callback. Use `Cache::store(Some("redis"))?.funnel(name)?` to select a named store. Stores without atomic locks return `CacheError::LocksUnsupported`.

### Clearing cache locks {#clearing-cache-locks}

Use `Cache::flushLocks().await?` for the default store, or `Cache::store(Some("database"))?.flushLocks().await?` for a named store. Artisan accepts the same optional store name.

```sh
./artisan cache:clear database --locks
./artisan cache:clear redis --tags=reports,daily
```

The lock command clears all locks in the selected lock store, including locks from other prefixes that share that backend. Array and file stores keep lock records separate from value records. Database stores require a separate lock table. Redis clears the entire dedicated lock database. Use a different database number on a shared Redis server. For different addresses with the same database number, OpenRust checks the Redis server IDs before clearing. The connection must permit `INFO SERVER` in that case.

Lock-only clearing supports a dedicated standalone database or a dedicated cluster. Redis flushes the entire lock backend. The cache and lock connections must point to different databases or clusters. When a cluster is involved, allow `CLUSTER INFO`, `CLUSTER NODES`, and `INFO CLUSTER` on a standalone peer so OpenRust can detect aliases before clearing. [Redis cluster setup](../database/redis.md)

`--tags` clears matching tagged values. Combining nonempty `--tags` with `--locks` fails.

## Redis Store

Set `CACHE_STORE=redis` to use the [Redis connections](../database/redis.md) for caching. Cache pull uses the Redis `GETDEL` command. Redis 6.2 or later is required for that operation.

`cache:clear redis` clears the entire logical database selected by the Redis cache store, including keys from other prefixes in that database. A lock connection using a separate database retains its locks.

## Cache Failover {#cache-failover}

Set `CACHE_STORE=failover` to make `failover` the default cache store. In `config/cache.rs`, the `stores` list for that driver defines the order. The application defaults to `database` followed by `array`. Use `["redis", "database", "array"]` to try Redis first.

An operation starts with the first store each time. A store error advances to the next store. A cache miss or a successful `false` result ends the operation. When the primary recovers, reads start there again, so an entry written to a fallback during an outage may be absent. If every store fails, the operation returns the last error.

Listen to `openrust::cache::events::CacheFailedOver` in the application dispatcher for newly failed stores. The event carries `storeName` and an `exception` message. Repeated failures of the same store only emit again after an operation in which it did not fail. A lock handle uses the first store that can create it. Acquiring, renewing, and releasing that handle all use the selected store.

```rust
use openrust::{
    cache::{CacheManager, CacheResult, Value},
    contracts::cache::Repository as _,
};

pub async fn cache_status(manager: &CacheManager) -> CacheResult<Option<Value>> {
    manager.store(Some("failover"))?.get("status").await
}
```

## Adding Custom Cache Drivers {#adding-custom-cache-drivers}

Implement `openrust::contracts::cache::Store` for your backend. Register the driver during a service provider's `boot` method, before resolving any store that uses it. This example registers a driver backed by an isolated in-memory store.

```rust
use std::sync::Arc;

use openrust::{cache::ArrayStore, support::facades::Cache};

Cache::extend("project", |manager, config| {
    manager.repository(Arc::new(ArrayStore::new()), config)
})?;
```

Set the named store's `driver` to `project` in `config/cache.rs`. The callback receives the manager and the full store configuration. Use `manager.getContainer()` for application services, and `manager.repository(..., config)` to retain the store name and event settings. Store resolution is synchronous. Construct the backend in the callback and perform asynchronous I/O in its cache operations.

The manager creates a named repository on first use and reuses it. Replacing a creator takes effect for the next resolution after `Cache::forgetDriver(Some("reports"))?` or `Cache::purge(Some("reports"))?`. Use `Cache::build(config)?` for a repository with its own configuration and lifetime.

## Cache Events {#cache-events}

Register listeners for cache events such as `CacheHit::NAME`, `KeyWritten::NAME` and `CacheLocksFlushed::NAME` through the application event dispatcher. Events in `openrust::cache::events` carry the selected store name. Read and write events expose the developer's key and ordered tags where applicable. Bulk reads dispatch `RetrievingManyKeys`, followed by a hit or miss for each requested key.

Set `events: false` on an individual store in `config/cache.rs` to disable its repository events. Failover repositories default to disabled and still use the event settings of each backing store. Memoized repositories forward events when they access the backing store. Repeated reads served from the memoized snapshot don't produce another event.

If a synchronous listener fails, the operation returns its error. An error from a listener before a write prevents the write. An error after the write leaves the stored value in place. For atomic `pull`, the retrieval and removal pre-events both run before the store operation.

To assert a cache hit in a test, seed `reports/latest` and call the repository from a booted `TestApp`:

```rust
use openrust::{cache::events::CacheHit, contracts::cache::Repository as _};

let events = app.fakeEvents(&[CacheHit::NAME]).await?;
let _ = cache.get("reports/latest").await?;
events.assertDispatched(CacheHit::NAME);
```
