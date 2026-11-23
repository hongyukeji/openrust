# Database Settings (Extension)

`openrust-settings` stores typed application settings in a database. Groups may represent website information, delivery services or application-specific policies. It reuses OpenRust's database, encryption, migration and scoped container services.

## Enable and migrate

Enable the optional package with the `settings` Cargo feature. Package discovery registers its provider and default table migration; it does not register application-specific settings groups:

```sh
cargo run --features settings -- artisan migrate
```

The package supplies its own defaults; the default application contains no `config/settings.rs`. To customize them, run `./artisan vendor:publish --tag=settings-config`, add `mod settings;` and `("settings".to_owned(), settings::load(env)?),` to the module declarations and configuration map in `config/mod.rs`, then rebuild. Preserve your custom table migration when changing the table or connection.

For another application, add the `openrust-settings` dependency and use OpenRust package discovery. Declare your own settings type, then call `SettingsServiceProvider::register_settings::<YourSettings>(app).await?` in that application's provider after the package registers. The default application contains no pre-registered `SiteSettings` group. Run migrations before reading settings; provider registration and boot do not query the settings table.

`settings.repository` defaults to `database`; `settings.repositories.database.connection` defaults to the application's default connection and `table` to `settings`. For a custom connection or table, register a corresponding `CreateSettingsTable` migration and run it on that connection instead of installing the default migration. This is a compiled migration in the existing ledger, not a second migration runner.

## Declare and use a group

Implement `SettingsGroup` on a struct deriving `Clone`, `Serialize` and `Deserialize`. `group()` supplies a unique group name, `defaults(config)` supplies the complete object schema, `validate()` checks business constraints and `encrypted()` lists sensitive top-level properties. Keep the serialized field set stable; do not conditionally omit fields. Group/property names use unqualified ASCII identifiers, at most 128 bytes. Names starting with `__settings_` are reserved for internal properties.

Resolve `Settings<YourSettings>` asynchronously from the application/container in a request or job scope. Trusted application code can then use:

```rust,ignore
let settings = app.make::<Settings<SiteSettings>>().await?;
let current = settings.get().await;
settings.fill(serde_json::json!({"title": "My website"})).await?;
settings.save().await?;
settings.reset("title").await?;
settings.save().await?;
```

`get()` returns a typed copy. Change values through `fill`, then explicitly `save`. `fill` rejects undeclared fields and invalid types, and validates the complete candidate before accepting any edit. Nested objects and arrays replace the entire property; backend forms must explicitly merge permitted nested patches. `reset` removes the database override and uses its declared default. An omitted property stays unchanged; `null`, empty strings, zero and false remain distinct values when allowed by the type. `refresh()` discards local edits and reloads the group.

`lock`/`unlock` apply to persisted properties after pending edits are saved. Attempts to change a locked property fail, rather than silently succeeding. Every group uses one reserved `__settings_version` row to serialize writes and detect stale editors, including first creation and reset. This is an intentional extension of Spatie's table layout. `Conflict` means refresh and resolve the edit before retrying; do not retry a stale snapshot blindly. Older schemas preserve unknown stored properties.

Module settings use the same API. Declare the type inside the module, then call `SettingsServiceProvider::register_settings::<DeliverySettings>(app).await?` from its Provider after the settings Provider registers. The group shares the settings table and remains separate from `site`; no repository changes are required.

```rust
use openrust::contracts::config::Repository;
use openrust_settings::{SettingsGroup, SettingsResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct DeliverySettings { enabled: bool }
impl SettingsGroup for DeliverySettings {
    fn group() -> &'static str { "module_delivery_options" }
    fn defaults(_: &dyn Repository) -> SettingsResult<Self> {
        Ok(Self { enabled: true })
    }
}
```

## Transactions and lifecycle

`save()` updates the instance's persisted snapshot after successful commit. `save_in(&mut transaction)` uses a savepoint in a caller-owned transaction and does not publish a new persisted snapshot. Supply the intended database transaction; refresh after the outer commit. After rollback the pending edit can be retried. If an operation is cancelled, its owner must roll back or drop the transaction.

A group uses a versioned process snapshot within the existing request/job scope. With `SettingsServiceProvider`, the next scope sees changes after reconciliation (60 seconds by default), rather than querying all properties on every request. Existing scopes and unsaved edits keep their original snapshot. Use `refresh()` to bypass that interval and discard local edits. Long-running commands, scheduler iterations and streams must define their refresh boundary. Multiple groups do not form one global snapshot. Database failures and malformed data fail explicitly when checked; only missing overrides use defaults.

The default repository bounds groups to 256 properties and 1 MiB of persisted JSON, excluding small internal metadata overhead. No shared cache is required. Static `Config` remains unchanged; database settings are not embedded in `config:cache`.

## Single instance and cluster

The same `Settings<T>` API, provider and configuration work on one server or multiple application/Worker processes. All nodes share the authoritative settings database and table. No cluster flag, Redis requirement or separate watcher command is needed; the first actual read starts a lightweight in-process checker. Merely registering a group does not read its table.

Published `config/settings.rs` contains these optional settings:

```rust,ignore
"namespace": null,
"refresh_interval": 60,
"cache": {
    "enabled": true,
    "store": null,
    "ttl": 3600,
},
```

`refresh_interval` is in seconds (default 60; 1–86400 through the provider). Set it to **0** to disable background checks and automatic expiry of loaded snapshots. After the initial load, reads reuse the snapshot until an explicit `refresh()`, `poll()` or `notify()` updates it, or a local save updates it. Automatic handler retries are also stopped; drive them with `poll()` or `notify()` when needed. Cache enablement is independent. Concurrent reads of the same group reuse the completed version check instead of queuing identical queries. A minute is a conservative default for infrequently changed settings, not a capacity guarantee: account for active processes and groups when sizing the database. `cache.store = null` reuses the application's default Cache store. The configured store must exist; set `cache.enabled` to `false` to omit cache acceleration. The default namespace combines environment, database driver, database name, table prefix and table. Keep it stable and identical across nodes. Independent applications sharing a Cache must use distinct namespaces and separate settings tables or connections. A namespace isolates cache/notifications, **not database rows**; set it explicitly when the default identity cannot distinguish deployments or database schemas.

The checker batch-reads persistent group versions from the **write connection**, only for groups used by this process. Changed groups reload their complete snapshot; unchanged groups do not reload property values. The existing Cache stores a versioned raw snapshot and a metadata-only change hint, with at most two keys per group/namespace. It does not create a new cache key on each update. Encrypted properties remain ciphertext in the shared snapshot. Hints are checked at up to one-second intervals; database reconciliation still runs without shared cache, with a local cache, or after missed hints, expired entries and Redis restarts. This is eventual convergence, not simultaneous activation on every node. Slow databases or application handlers can extend the normal interval.

Database commits are authoritative: cache timeouts or failed handlers cannot turn a committed save into a rollback error. Cache I/O is bounded and failures temporarily bypass cache; `cache_failures()` exposes the process counter. Database failures invalidate the checked state and fresh loads fail until recovery, while existing scoped values remain usable. New processes load the latest committed state; no notification replay is required. Use the repository's write APIs so every update changes its persistent version. Direct SQL edits that keep the same version require explicit `refresh()`.

`save_in` publishes its hint only after the outer transaction commits; rollback publishes nothing. A crash between commit and hint is repaired by the next database check. Each callback is deduplicated per observed version in that process; failed callbacks retry. Intermediate versions may be coalesced. Handlers must be idempotent, and process restarts do not provide durable or exactly-once business-event delivery.

### Refresh application consumers

Register application-owned invalidation/rebuild logic through `SettingsManager`. Obtain it with `app.make::<SettingsManager>().await?` after package registration. For example, invalidate an application-owned cached projection:

```rust,ignore
use openrust::{cache::CacheManager, contracts::cache::Repository as _};
use openrust_settings::{SettingsError, SettingsManager};

let manager = app.make::<SettingsManager>().await?;
let cache = app.make::<CacheManager>().await?.store(None)?;
let handler = manager.on_change("site", move |_change| {
    let cache = cache.clone();
    Box::pin(async move {
        cache.forget("site:rendered").await
            .map_err(|_| SettingsError::Refresh)?;
        Ok(())
    })
}).await?;

let state = manager.status("site").await;
// state: observed_version, applied_version, pending_handlers, last_error
manager.forget_handler("site", handler).await;
```

Callbacks run in independent async tasks, without the current HTTP/job facade scope. Capture the dependencies they need. When a callback needs the manager itself, capture `manager.downgrade()` and call `upgrade()` inside it to avoid a reference cycle. The five-second callback timeout records a redacted error and leaves the version pending; while automatic checks are enabled, callbacks retry even when no newer version appears. Initial loading establishes a baseline and does not fire a change callback. `status()` reports this process's registered handlers, **not acknowledgement from every cluster node**. Do not use it as a deployment-wide barrier or a durable audit trail.

`poll().await?` explicitly reconciles all active groups; `notify(&SettingsNotification { namespace, group, version }).await?` accepts metadata hints from an optional application transport. It verifies against the database, ignores other namespaces/inactive groups and never trusts an incoming value. Neither method requires Redis Pub/Sub. `shutdown().await` stops this manager's background task without closing shared connections. Explicit load/refresh/poll remain available afterwards.

Outside the application provider, compose `SettingsManager::new(source, SettingsOptions::default(), None)?`, wrap it in `Arc`, and pass it to `Settings::<T>::load`. Set a stable namespace for the deployment. A custom `SettingsRepository` can override `versions()` for efficient batch checks; its default falls back to complete reads. Nonempty snapshots must carry a nonempty opaque version via `SettingsSnapshot::with_version`, changed on every commit. Empty never-persisted groups may have no version.

This feature does not distribute `.env` files or mutate `Config`, deployment keys, listening ports or database pools. Service-specific refresh boundaries and rolling deployment remain explicit application responsibilities.

## Credentials and backend integration

Encrypted properties use the application encrypter and its previous-key ring. Nested credentials can be one encrypted object. `re_encrypt()` rewrites existing unlocked encrypted properties under the current key; retain previous keys until rewriting and recovery checks finish. Restore both database values and the required deployment keys. Damaged ciphertext never silently falls back to defaults.

The wrapper cannot be serialized and its debug output omits values. The typed value returned by `get()` is trusted internal data: build an explicit public or masked projection for HTTP, logs and queue payloads. Backend authorization, validation, secret-preserving form updates and persistent audit belong to the application.

The extension does not install backend routes or dynamically reconfigure SMS, mail or filesystem managers. Those consumers require explicit adapters to rebuild cached service instances. Existing operations retain their snapshots; historical uploads must retain their disk identity. Database translation entries remain the responsibility of the [translation loader](../digging-deeper/localization.md).
