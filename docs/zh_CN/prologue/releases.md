# 发布说明

## 版本与上游基线

当前源码在 `Cargo.toml` 中声明开发版 `0.0.0`。构建时使用已提交的 `Cargo.lock`，部署时记录源码提交号。OpenRust 版本号描述 OpenRust 自身的版本。Laravel v13.40.0 是兼容基线，应用启动与装配参考 Goravel v1.18.0。 [Laravel 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 源码](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## 已验收环境

| 环境 | 验收范围 |
| --- | --- |
| macOS ARM64、Rust 1.94.0 | 源码安装、独立部署、HTTP 和 Artisan |
| Linux ARM64、Debian Bookworm 容器、Rust 1.94.0 | 以非特权用户运行独立产物、HTTP、SQLite 迁移和 Artisan |
| Linux ARM64、Debian Bookworm 容器、LinuxKit 7.0.14 | 默认同进程启动、SQLite 队列与独立 Artisan 对照、SIGTERM 优雅退出 |
| SQLite | 新应用迁移和独立应用启动 |
| MySQL 8.4 | macOS ARM64 独立产物迁移及框架 Eloquent 集成验收 |
| Redis 8.10 | 队列验收及两个进程间的 Reverb 发布 |
| Linux x86_64 和 Windows 二进制 | 目标平台实机验收待完成 |

各平台与驱动的验收范围见表，功能边界见相应指南，例如 [Reverb 扩展](../packages/reverb.md)。独立产物的构建和运行见[部署](../getting-started/deployment.md)。

## 变更

应用开发者可在仓库的[更新日志](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看变更。迁移已有应用前请查阅[升级指南](upgrade.md)。
