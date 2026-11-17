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
| MySQL 5.7.44, Oracle Linux 7 derived image | Verified TLS 1.2 with ECDHE, 12 migrations, idempotent rerun, HTTP, database sessions and Artisan job processing in amd64 compatibility mode |
| MySQL 8.0.46, 8.4.13 | Isolated databases, 12 migrations, idempotent rerun, Artisan database worker, HTTP and database session writes |
| MariaDB 10.3.39, 11.8.9 | Verified TLS, 12 migrations, idempotent rerun, HTTP, database sessions, JSON and Schema checks, and Artisan job processing |
| SQL Server 2017 (14.0.3570.2), 2022 (16.0.4325.4) | Rust TDS with verified TLS, 12 migrations, idempotent rerun, HTTP, database sessions and Artisan job processing with Linux amd64 server containers in a compatibility environment |
| PostgreSQL 10–18 | Isolated databases, 12 migrations, idempotent rerun, Artisan database worker, HTTP and database session writes. Job dispatch and reservation were also exercised on 10 and 18 |
| Redis 8.10 | Queue acceptance and Reverb publication between two processes |
| Ubuntu 24.04.3 LTS x86_64, Docker Engine 29.9.3 | Native Rust 1.94.0 release build from `07307c6e1`, Compose with MySQL and Redis, 12 migrations, real HTTP and Artisan, a Redis job, shared database and cache across two app instances, storage persistence and SIGTERM shutdown |
| Docker Desktop on Apple Silicon, amd64 compatibility mode | SQLite, Artisan, HTTP, persistent storage and SIGTERM shutdown |
| Windows 11 Pro ARM64 build 26200.9457 in Parallels | Native ARM64 MSVC release executable. SQLite migrations, Artisan, welcome and health routes, public files, relocation and graceful Ctrl+C passed. Docker Desktop backend unverified |

The platform and driver rows cover the scenarios listed. Check the corresponding feature guide for its scope, such as [Reverb scaling](../packages/reverb.md). For standalone artifacts, follow [Deployment](../getting-started/deployment.md).

The database rows record release acceptance. PostgreSQL 10–18 results apply to the tested patch versions. The original Oracle Linux 7 image requires the [ECDHE-enabled derived image](../database/getting-started.md#mysql-57-oracle-linux-7) for OpenRust. See [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls), [MariaDB](../database/getting-started.md#mariadb) and [SQL Server](../database/getting-started.md#sql-server) for tested connection settings.

## Pending deployment acceptance {#pending-deployment-acceptance}

| Target | Environment or access needed |
| --- | --- |
| Windows Docker Desktop | A reachable Windows host with Docker Desktop running Linux containers. The test account must run `docker info`, `docker compose version` and `docker buildx version` and have read/write access to an isolated Windows test directory for the bind mounts. The available Parallels ARM64 guest has no Docker Desktop or usable WSL 2 backend. |
| Production TLS | The actual HTTPS URL and deployed hostname, a trusted certificate chain, and access to run read-only Artisan commands for the same deployment. TLS 1.2 and 1.3 have passed on the local proxy with a test CA. |
| Managed AWS SQS Standard and FIFO | Two isolated queue URLs, one Standard and one FIFO, their AWS region, an isolated MySQL batch ledger, and credentials scoped to the queues. The core checks use `sqs:SendMessage` (including batch sends), `sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility`, and `sqs:GetQueueAttributes`. Include the session token if using temporary credentials. |

Queue permissions are listed in the [AWS SQS API reference](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html).

## Changes

The repository [changelog](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md) records application-facing changes. See the [Upgrade Guide](upgrade.md) before moving an existing application to this source line.
