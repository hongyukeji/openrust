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

## Standalone Package Recheck {#standalone-package-recheck}

On macOS ARM64, a clean archive of `2d1f83dfd` built the standalone release binary with Rust 1.94.0 and the committed lockfile. The release directory ran from a separate working directory with MySQL 8.4.13 and Redis 8.10.3. Key generation, 12 migrations, an idempotent rerun, the home and health routes, both public file mounts, database sessions, both managed and independent Redis workers, and graceful SIGTERM all passed. Moving the release directory preserved access to public storage and the existing migration ledger.

## Production simulation and live deployment checks {#pending-deployment-acceptance}

Release acceptance for 08-04 is complete under the production simulation scope. The live deployment checks below cover each target environment.

| Target | Local acceptance | Live deployment checks |
| --- | --- | --- |
| Windows Docker Desktop | Windows PowerShell parsed Compose profiles and bind paths with spaces. The Windows ARM64 standalone build passed HTTP and Artisan. | A reachable Windows host running the Docker Desktop Linux-container backend and an account able to use Docker, Compose and Buildx and read/write a Windows test directory. Verify a clean build and deployment, read-only public files, persistent writable storage, HTTP and Artisan. |
| Production TLS | A local proxy verified TLS 1.2/1.3, hostname and CA checks, rejection of a missing intermediate, and leaf rotation on the same endpoint with real HTTP and a Secure session cookie. | Verify the real HTTPS URL, deployed hostname, served chain and protocol policy, the welcome and health routes, and read-only Artisan for the same deployment. |
| Managed AWS SQS Standard and FIFO | ElasticMQ covered delivery semantics and batch recovery. A separate loopback verifier recomputed SigV4 on real Artisan send, metrics, receive and delete requests for both queues and rejected a missing session token and an invalid signature. | Two isolated queue URLs, one Standard and one FIFO, their region, scoped IAM credentials and an isolated MySQL batch ledger. Verify AWS-side IAM and temporary credentials, queue metrics, redelivery, FIFO deduplication and recovery with real workers. |

Scope the test credentials to `sqs:SendMessage` (including batch sends), `sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility` and `sqs:GetQueueAttributes`. Include the session token for temporary credentials.

Queue permissions are listed in the [AWS SQS API reference](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html).

## Changes

The repository [changelog](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md) records application-facing changes. See the [Upgrade Guide](upgrade.md) before moving an existing application to this source line.
