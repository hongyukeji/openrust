# Cache

## Configuration

The default store is selected by `CACHE_STORE` in `config/cache.rs`. Use `array` for application-local memory, `file` for a directory shared by local processes, or `database` for a shared SQLite or MySQL store. The default application migration creates the `cache` and `cache_locks` tables. Run it against the configured database before using the database store.

```sh
./artisan migrate
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

## Redis Store

Set `CACHE_STORE=redis` to use the [Redis connections](../database/redis.md) for caching. Cache pull uses the Redis `GETDEL` command. Redis 6.2 or later is required for that operation.

`cache:clear redis` clears the entire logical database selected by the Redis cache store, including keys from other prefixes in that database. A lock connection using a separate database retains its locks.
