# Release Notes

## Version and upstream baseline

The current source tree declares development version `0.0.0` in `Cargo.toml`. Build with the committed `Cargo.lock` and keep the source commit with your deployment record. OpenRust version numbers describe OpenRust's own releases. Laravel v13.40.0 defines the compatibility baseline, and Goravel v1.18.0 informs application startup and assembly. [Laravel source](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel source](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## Verified environments

| Environment | Evidence |
| --- | --- |
| macOS ARM64, Rust 1.94.0 | Source installation and standalone deployment, HTTP and Artisan |
| Linux ARM64, Debian Bookworm container, Rust 1.94.0 | Standalone deployment, HTTP, SQLite migrations and Artisan under an unprivileged user |
| SQLite | Fresh application migration and standalone application startup |
| MySQL 8.4 | Standalone migrations on macOS ARM64 and framework Eloquent integration acceptance |
| Redis 8.10 | Queue acceptance and Reverb publication between two processes |
| Linux x86_64 and Windows binaries | Native target acceptance pending |

The platform and driver rows cover the scenarios listed. Check the corresponding feature guide for its scope, such as [Reverb scaling](../packages/reverb.md). For standalone artifacts, follow [Deployment](../getting-started/deployment.md).

## Changes

The repository [changelog](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md) records application-facing changes. See the [Upgrade Guide](upgrade.md) before moving an existing application to this source line.
