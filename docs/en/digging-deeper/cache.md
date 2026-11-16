# Cache

## Configuration

The default store is selected by `CACHE_STORE` in `config/cache.rs`. Use `array` for application-local memory, `file` for a directory shared by local processes, or `database` for a shared SQLite or MySQL store. The default application migration creates the `cache` and `cache_locks` tables. Run it against the configured database before using the database store.

```sh
./artisan migrate
```

Session data can use the [session cache](../the-basics/session.md#session-cache). The `session` store requires an active HTTP session and provides neither tags nor cache locks.

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

Run `./artisan cache:prune-stale-tags redis` to remove sorted-set references to expired or deleted values. This command preserves live values. Pass a store name, or omit it to use the default store.

## Atomic Locks

Use `Cache::lock` to create a lock handle. `get` tries once and `block` waits for a bounded interval. An owner can release or refresh a live lock. To release it from another process, pass the owner token to `Cache::restoreLock`. Use a finite lease to recover if the process ends before release.

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

### Clearing cache locks {#clearing-cache-locks}

Use `Cache::flushLocks().await?` for the default store, or `Cache::store(Some("database"))?.flushLocks().await?` for a named store. Artisan accepts the same optional store name.

```sh
./artisan cache:clear database --locks
./artisan cache:clear redis --tags=reports,daily
```

The lock command clears all locks in the selected lock store, including locks from other prefixes that share that backend. Array and file stores keep lock records separate from value records. Database stores require a separate lock table. Redis clears the entire dedicated lock database. Use a different database number on a shared Redis server. For different addresses with the same database number, OpenRust checks the Redis server IDs before clearing. The connection must permit `INFO SERVER` in that case.

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
