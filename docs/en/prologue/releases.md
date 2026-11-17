# Release Notes

## Version and upstream baseline

The current source tree declares development version `0.0.0` in `Cargo.toml`. Build with the committed `Cargo.lock` and keep the source commit with your deployment record. OpenRust version numbers describe OpenRust's own releases. Laravel v13.40.0 defines the compatibility baseline, and Goravel v1.18.0 informs application startup and assembly. [Laravel source](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel source](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## Verified environments

| Environment | Evidence |
| --- | --- |
| macOS ARM64, Rust 1.94.0 | Source installation and standalone deployment, HTTP and Artisan |
| Linux ARM64, Debian Bookworm container, Rust 1.94.0 | Standalone deployment, HTTP, SQLite migrations and Artisan under an unprivileged user |
| Linux ARM64, Debian Bookworm container, LinuxKit 7.0.14 | Default same-process startup, SQLite queue parity with Artisan and graceful shutdown on SIGTERM |
| SQLite 3.51.3 | Clean checkout, 12 application migrations, Artisan and HTTP |
| MySQL 5.7.44, Oracle generic Linux archive | TLS 1.2 with certificate and server-name verification, 12 migrations, idempotent rerun, HTTP, database sessions and Artisan job processing |
| MySQL 8.0.46, 8.4.13 | Isolated databases, 12 migrations, idempotent rerun, Artisan database worker, HTTP and database session writes |
| PostgreSQL 10–18 | Isolated databases, 12 migrations, idempotent rerun, Artisan database worker, HTTP and database session writes. Job dispatch and reservation were also exercised on 10 and 18 |
| Redis 8.10 | Queue acceptance and Reverb publication between two processes |
| Ubuntu 24.04 x86_64 and Docker Desktop on Apple Silicon | Earlier native amd64 container acceptance on Ubuntu. The fresh release was rebuilt and ran in Docker Desktop amd64 compatibility mode with SQLite, Artisan, HTTP, persistent storage and SIGTERM shutdown |
| Windows 11 Pro ARM64 build 26200.9457 in Parallels | Native ARM64 MSVC release executable. SQLite migrations, Artisan, welcome and health routes, public files, relocation and graceful Ctrl+C passed. Docker Desktop backend unverified |

The platform and driver rows cover the scenarios listed. Check the corresponding feature guide for its scope, such as [Reverb scaling](../packages/reverb.md). For standalone artifacts, follow [Deployment](../getting-started/deployment.md).

The database rows record release acceptance. PostgreSQL 10–18 results apply to the tested patch versions. The MySQL 5.7.44 Oracle Linux 7 container build failed ECDHE negotiation with OpenRust's TLS client. See [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls) for the tested server configuration. The MariaDB and SQL Server connection names are reserved. Both currently return an unavailable driver.

The available Windows ARM64 guest lacks nested virtualization and a working WSL 2 backend. Its standalone deployment is verified. The Windows Docker Desktop run and a native Ubuntu rerun of the current release remain pending.

TLS 1.2 and 1.3 passed through a local reverse proxy using a test CA, with certificate chain and hostname verification. Validation against a production endpoint is pending.

Managed AWS SQS Standard and FIFO deployment validation is pending. ElasticMQ evidence is recorded with the queue implementation.

## Changes

The repository [changelog](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md) records application-facing changes. See the [Upgrade Guide](upgrade.md) before moving an existing application to this source line.
