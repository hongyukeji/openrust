# Release Notes

## Version and upstream baseline

The framework and official packages released together inherit `workspace.package.version` from the root `Cargo.toml`. The current source version comes directly from that manifest:

<<< ../../../Cargo.toml#framework-version{toml}

To prepare a release, change this one value, run `cargo metadata --format-version 1` to refresh `Cargo.lock`, review the lockfile changes, then run the checks and update the changelog. Build releases with the committed lockfile and record the source commit. Historical release notes keep their original version numbers.

Rebuild after changing the package version. If the application uses cached configuration, regenerate it to refresh the default `app.version`; an explicit `APP_VERSION` remains the application's setting.

The repository's default application also inherits this version. A business application may instead declare its own `[package].version`; `app.version` defaults to that application's Cargo package version and can be overridden with `APP_VERSION`. `App::version()` and Artisan `--version` report the framework package version. Third-party vendored packages retain their upstream versions. The documentation's `master` / `v0.x` menu identifies source lines, rather than the application's version.

Packages currently declare `publish = false`; this version arrangement does not enable registry publishing. Source installations retain the workspace manifest; Cargo-generated `.crate` archives resolve inherited metadata into their own manifests. Copying a member directory alone does not include its workspace metadata. Container image tags remain configured through `APP_IMAGE_VERSION` as described in [Deployment](../getting-started/deployment.md).

## Environment compatibility

The source has been exercised on macOS ARM64, Linux ARM64/x86_64 and Windows ARM64. Build for the target operating system and architecture; binaries are not interchangeable between them. Windows Docker Desktop requires a working Linux-container backend.

The database versions covered by the existing driver checks are SQLite 3.51.3, MySQL 5.7.44/8.0.46/8.4.13, MariaDB 10.3.39/11.8.9, SQL Server 2017/2022 and PostgreSQL 10–18. This describes specific checked versions and scenarios, rather than every feature on every platform or patch version. See [Database](../database/getting-started.md) for driver configuration and version-specific constraints. Redis queue and Reverb integration has been exercised on Redis 8.10.

## Deployment requirements

Follow [Deployment](../getting-started/deployment.md) to build and run standalone artifacts. For MySQL 5.7, configure [TLS with ECDHE](../database/getting-started.md#mysql-57-tls); the Oracle Linux 7 image requires the [derived image](../database/getting-started.md#mysql-57-oracle-linux-7). See [affected-row counting](../database/queries.md#mysql-affected-rows) when porting database write logic.

Terminate production TLS at a reverse proxy or load balancer and verify the deployed hostname and certificate chain. Managed SQS requires the correct region, queue URLs and IAM credentials; local protocol tests do not establish AWS-side permissions. Configure queue-scoped `sqs:SendMessage`, `sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility` and `sqs:GetQueueAttributes` permissions, plus a session token for temporary credentials. See [Queues](../digging-deeper/queues.md).

## Changes

The repository [changelog](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md) records application-facing changes. See the [Upgrade Guide](upgrade.md) before moving an existing application to this source line.

## Dependency security

The locked build uses lettre 0.11.24, bundled SQLite 3.54.1 through libsqlite3-sys 0.39.1, and stable RSA 0.10.0. The SQLx SQLite adapter retains its accepted 0.9.0 implementation with a fixed SQLite dependency; the WebAuthn crypto adapter updates its RSA feature declaration and type imports. Their upstream source and licenses remain under `vendor/`. Passport keeps its published key generation and PEM formats, including explicit 1024-bit key generation; online signatures continue to use AWS-LC.

Date helpers use native Jiff 0.2.46 types and bundled IANA 2025b timezone data; SQL drivers use the official `time` integration. See [date helpers](../digging-deeper/helpers.md) for construction and calendar operations. The vendored image and AWS transport dependencies use upstream replacements for paste and pin-utils, with their source and licenses preserved. Audit the resolved dependencies and verify the target environment before releasing an application.
