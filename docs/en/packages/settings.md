# Database Settings (Extension)

`openrust-settings` stores typed application settings in a database. Groups may represent website information, delivery services or application-specific policies. It reuses OpenRust's database, encryption, migration and scoped container services.

## Enable and migrate

Enable the optional package with the `settings` Cargo feature. Package discovery registers its provider and default table migration; it does not register application-specific settings groups:

```sh
cargo run --features settings -- artisan migrate
```

The package supplies its own defaults; the default application contains no `config/settings.rs`. To customize them, run `./artisan vendor:publish --tag=settings-config`, add `mod settings;` and `("settings".to_owned(), settings::load(env)?),` to the module declarations and configuration map in `config/mod.rs`, then rebuild. Preserve your custom table migration when changing the table or connection.

For another application, add the `openrust-settings` dependency and use OpenRust package discovery. Declare your own settings type, then call `SettingsServiceProvider::registerSettings::<YourSettings>(app).await?` in that application's provider after the package registers. The default application contains no pre-registered `SiteSettings` group. Run migrations before reading settings; provider registration and boot do not query the settings table.

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

Module settings use the same API. Declare the type inside the module, then call `SettingsServiceProvider::registerSettings::<DeliverySettings>(app).await?` from its Provider after the settings Provider registers. The group shares the settings table and remains separate from `site`; no repository changes are required.

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

`save()` updates the instance's persisted snapshot after successful commit. `saveIn(&mut transaction)` uses a savepoint in a caller-owned transaction and does not publish a new persisted snapshot. Supply the intended database transaction; refresh after the outer commit. After rollback the pending edit can be retried. If an operation is cancelled, its owner must roll back or drop the transaction.

A group loads in one query from the write connection and is reused within the existing request/job scope. The next scope reads current database values. Do not retain settings in a permanent singleton. Long-running commands, scheduler iterations and streams must define an explicit refresh boundary. Multiple groups do not form one global snapshot. Database failures and malformed data fail explicitly; only missing overrides use defaults.

The default repository bounds groups to 256 properties and 1 MiB of persisted JSON, excluding small internal metadata overhead. No shared cache is required. Static `Config` remains unchanged; database settings are not embedded in `config:cache`.

## Credentials and backend integration

Encrypted properties use the application encrypter and its previous-key ring. Nested credentials can be one encrypted object. `reEncrypt()` rewrites existing unlocked encrypted properties under the current key; retain previous keys until rewriting and recovery checks finish. Restore both database values and the required deployment keys. Damaged ciphertext never silently falls back to defaults.

The wrapper cannot be serialized and its debug output omits values. The typed value returned by `get()` is trusted internal data: build an explicit public or masked projection for HTTP, logs and queue payloads. Backend authorization, validation, secret-preserving form updates and persistent audit belong to the application.

The extension does not install backend routes or dynamically reconfigure SMS, mail or filesystem managers. Those consumers require explicit adapters to rebuild cached service instances. Existing operations retain their snapshots; historical uploads must retain their disk identity. Database translation entries remain the responsibility of the [translation loader](../digging-deeper/localization.md).
