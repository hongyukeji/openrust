# Database Settings (Extension)

`openrust-settings` stores typed application settings in a database. Groups may represent website information, delivery services or application-specific policies. It reuses OpenRust's database, encryption, migration and scoped container services.

## Enable and migrate

For the application skeleton in this repository, add `settings` to the existing default feature list in `Cargo.toml` (preserve any other defaults). The `settings = ["dep:openrust-settings"]` declaration already exists:

```toml
[features]
default = ["settings"]
```

Now every `./artisan` invocation and application build enables the package, including the Worker. Do not rely on a one-off `cargo run --features settings`: feature selection is not saved for later commands. If you keep the feature optional, pass `--features settings` to **every** build/run instead.

Package discovery registers its provider and default table migration, not application-specific groups:

```sh
./artisan migrate
```

Use `./artisan` on macOS/Linux and `.\artisan.bat` in Windows CMD/PowerShell. Production automation uses the existing migration confirmation contract, for example `./artisan migrate --force`; verify the target database first.

The package supplies its own defaults; the default application contains no `config/settings.rs`. To customize them, run `./artisan vendor:publish --tag=settings-config`, add `mod settings;` and `("settings".to_owned(), settings::load(env)?),` to the module declarations and configuration map in `config/mod.rs`, then rebuild. Preserve your custom table migration when changing the table or connection.

For another application, add the `openrust-settings` dependency and use OpenRust package discovery. Declare your own settings type, then call `SettingsServiceProvider::register_settings::<YourSettings>(app).await?` in that application's provider after the package registers. The default application contains no pre-registered `SiteSettings` group. Run migrations before reading settings; provider registration and boot do not query the settings table.

`settings.repository` defaults to `database`; `settings.repositories.database.connection` defaults to the application's default connection and `table` to `settings`. For a custom connection or table, register a corresponding `CreateSettingsTable` migration and run it on that connection instead of installing the default migration. This is a compiled migration in the existing ledger, not a second migration runner.

## Declare and use a group

Implement `SettingsGroup` on a struct deriving `Clone`, `Serialize` and `Deserialize`. `group()` supplies a unique group name, `defaults(config)` supplies the complete object schema, `validate()` checks business constraints and `encrypted()` lists sensitive top-level properties. Keep the serialized field set stable; do not conditionally omit fields. Group/property names use unqualified ASCII identifiers, at most 128 bytes. Names starting with `__settings_` are reserved for internal properties.

### Define the type and register it

Create `app/settings.rs` with this complete group definition:

```rust
use openrust::contracts::config::Repository;
use openrust_settings::{SettingsGroup, SettingsResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SiteSettings {
    pub title: String,
    pub registration_enabled: bool,
}

impl SettingsGroup for SiteSettings {
    fn group() -> &'static str { "site" }

    fn defaults(config: &dyn Repository) -> SettingsResult<Self> {
        Ok(Self {
            title: config.get("app.name")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "My Site".into()),
            registration_enabled: false,
        })
    }
}
```

Add this declaration to `app/mod.rs`, preserving the existing modules:

```rust
pub(crate) mod settings;
```

In the existing `impl ServiceProvider for AppServiceProvider` in `app/providers/app_service_provider.rs`, merge the imports and the following logic into `boot`; preserve any existing boot logic. All provider registrations have completed at this point, so the package's group registry is available:

```rust,ignore
use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::ProviderResult,
};
use openrust_settings::SettingsServiceProvider;
use crate::app::settings::SiteSettings;

fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
    Box::pin(async move {
        SettingsServiceProvider::register_settings::<SiteSettings>(app).await?;
        Ok(())
    })
}
```

This only registers a lazy scoped binding; it does not query the table at boot. Do not register `SettingsServiceProvider` again in `bootstrap/providers.rs`, and do not resolve and retain `Settings<SiteSettings>` in a singleton. Rebuild after adding these Rust files; the source Artisan script rebuilds automatically.

### Read, update and restore defaults

The following `app` is the already bootstrapped `Application` supplied by your application integration. Execute the code inside the existing HTTP request or queue job scope:

```rust,ignore
use openrust_settings::Settings;
use crate::app::settings::SiteSettings;

let settings = app.make::<Settings<SiteSettings>>().await?;
let current = settings.get().await;
settings.fill(serde_json::json!({"title": "My website"})).await?;
settings.save().await?;
assert_eq!(settings.get().await.title, "My website");
settings.reset("title").await?;
settings.save().await?;
```

On a fresh group, `current.title` uses `app.name` (or `My Site` if absent). Saving changes the stored title; `reset("title")` removes the override and returns to that default. It does not delete the group or its concurrency anchor.

`get()` returns a typed copy. Change values through `fill`, then explicitly `save`. `fill` rejects undeclared fields and invalid types, and validates the complete candidate before accepting any edit. Nested objects and arrays replace the entire property; backend forms must explicitly merge permitted nested patches. `reset` removes the database override and uses its declared default. An omitted property stays unchanged; `null`, empty strings, zero and false remain distinct values when allowed by the type. `refresh()` discards local edits and reloads the group.

`lock`/`unlock` apply to persisted properties after pending edits are saved. Attempts to change a locked property fail, rather than silently succeeding. Every group uses one reserved `__settings_version` row to serialize writes and detect stale editors, including first creation and reset. This is an intentional extension of Spatie's table layout. `Conflict` means refresh and resolve the edit before retrying; do not retry a stale snapshot blindly. Older schemas preserve unknown stored properties.

Lock an explicitly persisted override after saving edits; inspect its lock state with `is_locked`, then unlock only through trusted application code:

```rust,ignore
settings.fill(serde_json::json!({"title": "Managed title"})).await?;
settings.save().await?;
settings.lock("title").await?;
assert!(settings.is_locked("title").await?);
settings.unlock("title").await?;
```

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

For example, when the settings repository uses the default connection, write inside that same database transaction (choose the configured connection instead if customized):

```rust,ignore
use openrust::database::DatabaseManager;

let database = app.make::<DatabaseManager>().await?;
let mut transaction = database.connection(None)?.begin_transaction().await?;
settings.fill(serde_json::json!({"registration_enabled": true})).await?;
settings.save_in(&mut transaction).await?;
transaction.commit().await?;
drop(transaction);
settings.refresh().await?;
assert!(settings.get().await.registration_enabled);
```

A group uses a versioned process snapshot within the existing request/job scope. With `SettingsServiceProvider`, the next scope sees changes after reconciliation (60 seconds by default), rather than querying all properties on every request. Existing scopes and unsaved edits keep their original snapshot. Use `refresh()` to bypass that interval and discard local edits. Long-running commands, scheduler iterations and streams must define their refresh boundary. Multiple groups do not form one global snapshot. Database failures and malformed data fail explicitly when checked; only missing overrides use defaults.

The default repository bounds groups to 256 properties and 1 MiB of persisted JSON, excluding small internal metadata overhead. No shared cache is required. Static `Config` remains unchanged; database settings are not embedded in `config:cache`.

## Single instance and cluster

The same `Settings<T>` API, provider and configuration work on one server or multiple application/Worker processes. All nodes share the authoritative settings database and table. No cluster flag, Redis requirement or separate watcher command is needed; the first actual read starts a lightweight in-process checker. Merely registering a group does not read its table.

### Configuration reference

| Key | Default | Purpose |
| --- | --- | --- |
| `repository` | `database` | Built-in persistent repository |
| `repositories.database.connection` | `null` | Application's default database connection |
| `repositories.database.table` | `settings` | Settings table; custom names need a matching migration |
| `namespace` | `null` | Infer a stable cache/notification identity from environment and database/table |
| `refresh_interval` | `60` | Database version-check interval in seconds; `0` disables automatic checks |
| `cache.enabled` | `true` | Optional cache acceleration; disabling this does not disable database checks |
| `cache.store` | `null` | Existing default Cache store; choose a configured named store if needed |
| `cache.ttl` | `3600` | Snapshot/hint lifetime in seconds (1–604800); not the refresh interval |

Only publish and register `config/settings.rs` when overriding package defaults. These are application-local configuration entries, not a second distributed configuration service. Rebuild and regenerate `config:cache` if your deployment uses cached configuration.

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

`refresh_interval` is in seconds (default 60; 1–86400 through the provider). Set it to **0** to disable background checks and automatic expiry of loaded snapshots. After the initial load, reads reuse the snapshot until an explicit `refresh()`, `poll()` or `notify()` updates it, or a local save updates it. Automatic handler retries are also stopped; drive them with `poll()` or `notify()` when needed. Cache enablement is independent. Concurrent reads of the same group reuse the completed version check instead of queuing identical queries. A minute is a conservative default for infrequently changed settings, not a capacity guarantee: account for active processes and groups when sizing the database.

`cache.store = null` reuses the application's default Cache store. The configured store must exist; set `cache.enabled` to `false` to omit cache acceleration. The default namespace combines environment, database driver, database name, table prefix and table. Keep it stable and identical across nodes. Independent applications sharing a Cache must use distinct namespaces and separate settings tables or connections. A namespace isolates cache/notifications, **not database rows**; set it explicitly when the default identity cannot distinguish deployments or database schemas.

The checker batch-reads persistent group versions from the **write connection**, only for groups used by this process. Changed groups reload their complete snapshot; unchanged groups do not reload property values. The existing Cache stores a versioned raw snapshot and a metadata-only change hint, with at most two keys per group/namespace. It does not create a new cache key on each update. Encrypted properties remain ciphertext in the shared snapshot. Hints are checked at up to one-second intervals; database reconciliation still runs without shared cache, with a local cache, or after missed hints, expired entries and Redis restarts. This is eventual convergence, not simultaneous activation on every node. Slow databases or application handlers can extend the normal interval.

Database commits are authoritative: cache timeouts or failed handlers cannot turn a committed save into a rollback error. Cache I/O is bounded and failures temporarily bypass cache; `cache_failures()` exposes the process counter. Database failures invalidate the checked state and fresh loads fail until recovery, while existing scoped values remain usable. New processes load the latest committed state; no notification replay is required. Use the repository's write APIs so every update changes its persistent version. Direct SQL edits that keep the same version require explicit `refresh()`.

`save_in` publishes its hint only after the outer transaction commits; rollback publishes nothing. A crash between commit and hint is repaired by the next database check. Each callback is deduplicated per observed version in that process; failed callbacks retry. Intermediate versions may be coalesced. Handlers must be idempotent, and process restarts do not provide durable or exactly-once business-event delivery.

### Disable automatic refresh

In the published configuration's `load` result, change:

```rust,ignore
"refresh_interval": 0,
```

Restart/redeploy every process after changing this compiled configuration. A node still loads a group on first use, but then keeps that snapshot without background checks or read-time expiry. Changes on other nodes no longer arrive automatically, even with Redis. Automatic callback retries also stop. Use these explicit boundaries when appropriate:

```rust,ignore
settings.refresh().await?; // Discards unsaved edits and reads the source immediately.
manager.poll().await?;     // Checks active groups and retries pending handlers.
```

`manager.notify(&notification).await?` also verifies a supplied metadata hint and runs pending handlers. `poll`/`notify` update the manager's snapshot for future scopes; they do not mutate a previously resolved `Settings<T>`. A local save still publishes and updates the local manager. After an outer `save_in` commit, refresh the editing instance explicitly.

### Deploy shared settings

1. Enable the same package and register the same group schemas in each application/Worker process.
2. Use the same settings database/table, namespace and applicable encryption keys on nodes belonging to one application. Apply migrations before reading groups.
3. Keep the default 60-second interval, or configure another positive interval. Existing shared Cache/Redis accelerates hints; without it the same database reconciliation still works.
4. Register consumer invalidation callbacks in **each process**, before its first actual group read. Initially unused groups have no cached consumer to refresh; registration alone does not activate polling for that group.
5. Start the application normally and the queue consumer with `./artisan queue:work`. No additional settings daemon is required. After saving on node A, a new scope on B sees the value after reconciliation; an old B scope stays unchanged until explicit refresh.

Independent applications sharing infrastructure must separate settings tables/connections as well as cache namespaces. Never copy one application's settings credentials or keys merely to make its namespace match another application.

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
```

Keep `handler` for as long as the consumer is active. Remove it only when that consumer is being detached or stopped, not immediately after registration:

```rust,ignore
manager.forget_handler("site", handler).await;
```

Callbacks run in independent async tasks, without the current HTTP/job facade scope. Capture the dependencies they need. When a callback needs the manager itself, capture `manager.downgrade()` and call `upgrade()` inside it to avoid a reference cycle. The five-second callback timeout records a redacted error and leaves the version pending; while automatic checks are enabled, callbacks retry even when no newer version appears. Initial loading establishes a baseline and does not fire a change callback. Successful source recovery clears the stale error once no handler is pending; a pending handler error remains until success or deregistration. Before the first actual group load, `status()` returns `None`. A never-persisted empty group also has no version; assess `pending_handlers` together with `last_error`, rather than treating a nonempty `applied_version` as the only readiness check. `status()` reports this process's registered handlers, **not acknowledgement from every cluster node**. Do not use it as a deployment-wide barrier or a durable audit trail.

`poll().await?` explicitly reconciles all active groups; `notify(&SettingsNotification { namespace, group, version }).await?` accepts metadata hints from an optional application transport. It verifies against the database, ignores other namespaces/inactive groups and never trusts an incoming value. Neither method requires Redis Pub/Sub. `shutdown().await` stops this manager's background task without closing shared connections. Explicit load/refresh/poll remain available afterwards.

Outside the application provider, compose `SettingsManager::new(source, SettingsOptions::default(), None)?`, wrap it in `Arc`, and pass it to `Settings::<T>::load`. Set a stable namespace for the deployment. A custom `SettingsRepository` can override `versions()` for efficient batch checks; its default falls back to complete reads. Nonempty snapshots must carry a nonempty opaque version via `SettingsSnapshot::with_version`, changed on every commit. Empty never-persisted groups may have no version.

This feature does not distribute `.env` files or mutate `Config`, deployment keys, listening ports or database pools. Service-specific refresh boundaries and rolling deployment remain explicit application responsibilities.

## Credentials and backend integration

Encrypted properties use the application encrypter and its previous-key ring. Nested credentials can be one encrypted object. `re_encrypt()` rewrites existing unlocked encrypted properties under the current key; retain previous keys until rewriting and recovery checks finish. Restore both database values and the required deployment keys. Damaged ciphertext never silently falls back to defaults.

The wrapper cannot be serialized and its debug output omits values. The typed value returned by `get()` is trusted internal data: build an explicit public or masked projection for HTTP, logs and queue payloads. Backend authorization, validation, secret-preserving form updates and persistent audit belong to the application.

The extension does not install backend routes or dynamically reconfigure SMS, mail or filesystem managers. Those consumers require explicit adapters to rebuild cached service instances. Existing operations retain their snapshots; historical uploads must retain their disk identity. Database translation entries remain the responsibility of the [translation loader](../digging-deeper/localization.md).

## Troubleshooting

| Symptom | Check |
| --- | --- |
| `settings-config` publication is unavailable | Ensure the package feature is enabled for this invocation/build, not just a previous command |
| Missing settings table | Run migrations on the configured connection; custom table/connection deployments need their own registered migration |
| Registration/binding fails | Register a unique group once, after package registration; reject undeclared properties and incorrect value types |
| Another node retains old values | Check interval `0`, database/table/namespace identity, package configuration cache and whether the node has read the group; start a new scope or explicitly refresh |
| Pending handlers | Inspect this process's `status()`; make callbacks idempotent and check captured dependencies; when automatic checks are off, call `poll()`/`notify()` explicitly |
| `Conflict` / `Locked` | Resolve the stale edit after refresh, or explicitly unlock with authorization; do not blindly retry/overwrite |
| Decryption or database error | Restore the correct keys/connection/data; these errors intentionally do not fall back to defaults |
