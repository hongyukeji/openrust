# Release Notes

## Version and upstream baseline

The current source tree declares development version `0.0.0` in `Cargo.toml`. Build with the committed `Cargo.lock` and keep the source commit with your deployment record. OpenRust version numbers describe OpenRust's own releases. Laravel v13.40.0 defines the compatibility baseline, and Goravel v1.18.0 informs application startup and assembly. [Laravel source](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel source](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## Environment compatibility

The source has been exercised on macOS ARM64, Linux ARM64/x86_64 and Windows ARM64. Build for the target operating system and architecture; binaries are not interchangeable between them. Windows Docker Desktop requires a working Linux-container backend.

The database versions covered by the existing driver checks are SQLite 3.51.3, MySQL 5.7.44/8.0.46/8.4.13, MariaDB 10.3.39/11.8.9, SQL Server 2017/2022 and PostgreSQL 10–18. This describes specific checked versions and scenarios, rather than every feature on every platform or patch version. See [Database](../database/getting-started.md) for driver configuration and version-specific constraints. Redis queue and Reverb integration has been exercised on Redis 8.10.

## Deployment requirements

Follow [Deployment](../getting-started/deployment.md) to build and run standalone artifacts. For MySQL 5.7, configure [TLS with ECDHE](../database/getting-started.md#mysql-57-tls); the Oracle Linux 7 image requires the [derived image](../database/getting-started.md#mysql-57-oracle-linux-7). See [affected-row counting](../database/queries.md#mysql-affected-rows) when porting database write logic.

Terminate production TLS at a reverse proxy or load balancer and verify the deployed hostname and certificate chain. Managed SQS requires the correct region, queue URLs and IAM credentials; local protocol tests do not establish AWS-side permissions. Configure queue-scoped `sqs:SendMessage`, `sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility` and `sqs:GetQueueAttributes` permissions, plus a session token for temporary credentials. See [Queues](../digging-deeper/queues.md).

## Changes

The repository [changelog](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md) records application-facing changes. See the [Upgrade Guide](upgrade.md) before moving an existing application to this source line.
