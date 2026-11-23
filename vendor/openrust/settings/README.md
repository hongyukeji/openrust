# OpenRust Settings

Optional typed database settings for OpenRust applications.
Application code declares groups; the repository persists overrides in `settings`.
The same Settings API supports single-instance and clustered applications through a database-backed version coordinator and optional acceleration using the existing Cache store. No backend routes or automatic service configuration overrides are installed.

```rust,no_run
use openrust::{contracts::config::Repository, config::Repository as Config};
use openrust_settings::{
    DatabaseSettingsRepository, Settings, SettingsGroup, SettingsManager,
    SettingsOptions, SettingsResult,
};
use serde::{Serialize, Deserialize};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
struct SiteSettings { title: String, registration_enabled: bool }
impl SettingsGroup for SiteSettings {
    fn group() -> &'static str { "site" }
    fn defaults(config: &dyn Repository) -> SettingsResult<Self> {
        Ok(Self {
            title: config.get("app.name").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or("My Site".into()),
            registration_enabled: false,
        })
    }
}

async fn example(connection: openrust::database::Connection, config: &Config) -> SettingsResult<()> {
    let repository = Arc::new(SettingsManager::new(
        Arc::new(DatabaseSettingsRepository::new(connection)),
        SettingsOptions { namespace: "my-application:production".into(), ..Default::default() },
        None,
    )?);
    let settings = Settings::<SiteSettings>::load(repository.clone(), config, None).await?;
    settings.fill(serde_json::json!({"title":"My website"})).await?;
    settings.save().await?;
    assert_eq!(settings.get().await.title, "My website");
    repository.shutdown().await;
    Ok(())
}
```

OpenRust package discovery registers `SettingsServiceProvider` when the optional
package is enabled. Its boot hook registers the default table migration through
`load_migrations_from`; run `artisan migrate` before reading settings. A custom table
or connection skips this default migration: register the corresponding
`CreateSettingsTable` yourself and run it on the intended connection.

Declare business settings in the consuming application and call
`register_settings::<T>(app)` in its provider after package registration. The default
application does not pre-register example groups. The container resolves `Settings<T>`
asynchronously once per existing scope; do not inject it into a permanent singleton.
Registration and boot need neither a settings table nor an encryption key for
nonsecret groups.

`fill` replaces declared top-level fields and validates the entire group. It rejects
unknown fields, bad types and locked properties. Nested objects and arrays are replaced
whole. Backend forms must explicitly merge allowed nested patches and retain omitted
secrets. `reset` removes an override; `null`, empty strings, zero and false remain values
when permitted by the declared Rust type. `refresh` explicitly discards local edits.
`lock`/`unlock` operate on persisted properties after pending edits have been saved.
Unlike Spatie's silent exclusion, attempted writes to locked properties return an error.

Each group has one reserved `__settings_version` row (not a user property), in addition
to Spatie's normal group/name/payload/locked/timestamp rows. It anchors transactional
locking and compare-and-save even for groups with no overrides. Reset never deletes
the anchor. No history table or external configuration service is required. All writes must
use this repository. Conflicts return `SettingsError::Conflict`; refresh and resolve
the edit deliberately. Older group schemas preserve unknown persisted properties.

`save_in` uses a savepoint in the supplied database transaction. The caller must select
the correct database/table and commit the outer transaction. It leaves the settings
instance unchanged as a database snapshot; refresh after commit. In-memory pending
edits remain visible through `get`, but the coordinator publishes a metadata hint only after the outer commit.
A failed or rolled-back outer transaction does not commit any settings changes.

Declare sensitive **top-level** properties with `encrypted()`. An entire object may
be encrypted; no nested-key encryption syntax is inferred. Supply the existing
application encrypter. `get` returns trusted internal data; settings wrappers cannot
be serialized and Debug output omits values. Never send the declared data struct
directly to clients or queue payloads. Build an explicit public/masked projection.
With previous keys installed, `re_encrypt` rewrites only existing unlocked encrypted
overrides under the current key. Keep old keys until rewriting and restore checks
complete. Missing keys and damaged ciphertext fail instead of falling back.

Raw reads use one bounded query on the write connection. The provider wraps this repository in `SettingsManager`, which batch-checks active group versions every 60 seconds and shares version-checked snapshots through the configured Cache store. Redis is optional; missed hints and cache failures recover through database checks. Missing tables and unavailable
databases fail; only absent properties fall back to application defaults. Maximum
group size is 256 properties and 1 MiB of persisted JSON. The reserved row is internal.
The application supplies authorization, persistent audit, public projections and
consumer refresh. Changes converge into future scopes; existing operations keep
their snapshots. Explicit `refresh()` always bypasses the interval. SMS, mail and filesystem managers need separate explicit adapters.

Module-owned settings use the same table and registration contract:

```rust,no_run
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

Package configuration defaults and the publishable `settings-config` template live in
the package. The default application does not ship `config/settings.rs`; publish
only when application overrides are needed, register its `load(env)` in the
application configuration map, and rebuild. Cached configuration remains authoritative.

`SettingsManager` exposes `on_change`, `forget_handler`, `status`, `poll`, `notify`
and `shutdown`. Change handlers are per-process, idempotent application adapters;
successful versions are deduplicated and failures retry. Status is not a cluster-wide
acknowledgement. Use `downgrade()` when retaining the manager in a handler. Cache
notifications contain namespace, group and version only, not setting values. The
namespace isolates cache metadata; independent applications also need separate tables
or connections to isolate their database values. The existing database layout is unchanged.

See [single-instance and cluster usage](../../../docs/en/packages/settings.md#single-instance-and-cluster)
for configuration, callback examples, custom repository contracts and lifecycle limits.

The default refresh interval is 60 seconds. Set `settings.refresh_interval` to `0`
(or `SettingsOptions.refresh_interval` to `Duration::ZERO`) to disable automatic
checks and snapshot expiry. Explicit refresh/poll/notify and local saves remain
available; handler retries then require explicit poll/notify as well. Concurrent
reads share the completed version check. Cache enablement remains independent.
