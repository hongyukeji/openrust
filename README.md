# OpenRust

> An opinionated, batteries-included Rust application framework. Create a project, run one command, and start writing business code.

**Status: v1 in active development.** Nothing is published to crates.io yet. The v1 scope includes configuration, HTTP conventions, PostgreSQL/MySQL, authentication and authorization, queue, scheduler, events, cache, storage, mail, CLI, testing tools and deployment support; planned capabilities are not complete until their implementation and acceptance evidence are recorded.

## Goals

- **Convention over configuration, without magic.** Directory layout, naming, routing, migrations and configuration keys follow visible conventions. Dependencies are explicit Rust types; there is no runtime container, no reflection, no facade.
- **One obvious way.** Each capability has one official entry point: `Validated<T>` for input, `Policy` for authorization, `Job` for background work.
- **Batteries included, drivers swappable.** The v1 plan uses a database queue, Redis cache, local storage and SMTP mail and pluggable SMS (Twilio adapter), with memory/file cache alternatives and optional S3-compatible storage. Both single-instance and clustered application deployments are planned; drivers are selected through Cargo features and configuration.
- **Generated code is ordinary code.** Everything `openrust generate` produces is readable, editable and testable Rust.
- **Testable by default.** Capabilities provide fake drivers, while the Mailpit Compose service supports real local SMTP verification; business tests do not require paid SMTP/SMS accounts or object storage.

OpenRust is built on `tokio`, `axum`, `tower` and `SeaORM`, with explicit Rust types and conventional project structure.

## Run in this repository

The non-published [integration server](apps/server/README.md) runs the currently implemented framework capabilities without external services:

```bash
cargo run -p openrust-server
curl --noproxy '*' http://127.0.0.1:3000/health
cargo run -p openrust-server -- config show
cargo run -p openrust-server -- help
cargo test -p openrust-server
```

For repository development, `.cargo/config.toml` sets `APP_CONFIG_DIR=apps/server/config`, so root-level `cargo run` reads the server's configuration. Applications default to `config/` relative to their working directory. `APP_SERVER__PORT=4000 cargo run` overrides the server's port; `PORT=4000 cargo run` also works. Applications can override `Application::config_namespace()` to use a prefix such as `SHOP_`. The framework package is at the repository root; implemented capabilities live in `src/audit.rs`, `src/config/`, `src/core/`, `src/db/`, `src/http/`, `src/i18n.rs`, `src/mail/`, `src/sms.rs`, and feature-gated `src/auth/` and `src/security.rs`. Runtime TOML/YAML remains in each application's root `config/` directory. `apps/server` is this workspace's integration host, while `openrust new` creates a separate project with its own `src/` and `config/`.

Application data encryption is an opt-in `encryption` feature. Register `openrust::security::EncryptionPlugin`, configure a 32-byte key ring under `[security.encryption]`, and generate local key material with `openrust key generate`. The active key encrypts new values; prior keys decrypt old values during a controlled rotation. Keep keys outside version control.

Purpose-bound expiring links are available through the `signing` feature and `openrust::signing::SignedLinks`. They use a separate key ring under `[security.signed_links]`; add `SignedLinkRevocationMigration` and a shared database store when individual links need cross-instance revocation. See [the signed-links guide](src/signing/README.md).

Explicit audit records use `openrust::audit::Audit` with the database sink or a custom `AuditSink`. Add `DatabaseAuditMigration` to the application migrator. Details are denied by default, require an allowlist, and redact sensitive field names. Critical database changes can insert a validated audit record in the same transaction; see the [audit module guide](src/audit/README.md).

The `queue` feature provides durable database job dispatch, bounded worker concurrency, and database-coordinated scheduling for PostgreSQL and MySQL. Register `QueueMigration` in the application migrator and install `QueuePlugin` after `DatabasePlugin`; handlers are explicitly registered with `register_job`. Use `schedule_interval` or `schedule_daily_at` on the same plugin to register a UTC schedule. `schedule run --once` scans once, `schedule run` runs continuously, and `schedule list` shows registered entries. `Queue::dispatch_unique` prevents overlapping duplicates for a queue-scoped key and returns the active job ID; the key is released when the job completes or is moved to `failed_jobs`. Dispatch inside a `Db` transaction when job creation must commit or roll back with business data. Execution is at least once, so handlers must be idempotent. The current worker supports retries, failed-job retention, manual retry, progress, cooperative cancellation, per-attempt timeouts and lease recovery after process failure. Schedules support fixed whole-second intervals and daily UTC time; named time zones, DST, cron expressions, queued event listeners, task chains, batches and cross-node full cluster acceptance remain in M4 work.

## The golden path

```bash
cargo install openrust-cli
openrust new blog --db postgres
cd blog
openrust dev
```

After `openrust dev` the database and mail catcher are running, migrations are applied, the server listens on `http://localhost:3000`, `/health` returns 200, `/docs` serves the OpenAPI UI, auth endpoints work and `cargo test` is green.

## Request languages

Generated applications install `I18nPlugin` by default. `[i18n] default_locale` and `fallback_locale` select the default language and final resource fallback; `Accept-Language` selects a supported request locale. The framework ships English and Simplified Chinese HTTP messages. Applications can merge JSON resources into `MessageCatalog`, use `RequestLocale` in handlers, or call `I18n::with_locale` for an explicit task scope. Templates support named `{name}` substitution; unknown keys stay visible as their key. This layer does not provide plural rules or locale-aware date/number formatting. See the [`I18n` API](src/i18n.rs).

```rust
use openrust::http::{Inject, Json};
use openrust::i18n::{I18n, MessageArgs, RequestLocale};

async fn greeting(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "app.greeting", &args))
}
```

Load an application JSON object into `MessageCatalog` at startup, for example `{"app.greeting":"Hello, {name}!"}` under the selected locale. Use the same explicit locale when composing an email or SMS outside a request.

## Documentation

Design and plan documents live in [`.agents/project/plans/`](.agents/project/plans/README.md) during the design phase and will move to a public documentation site before the 0.1 release.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
