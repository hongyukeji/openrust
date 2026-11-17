# 发布说明

## 版本与上游基线

当前源码在 `Cargo.toml` 中声明开发版 `0.0.0`。构建时使用已提交的 `Cargo.lock`，部署时记录源码提交号。OpenRust 版本号描述 OpenRust 自身的版本。Laravel v13.40.0 是兼容基线，应用启动与装配参考 Goravel v1.18.0。 [Laravel 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 源码](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## 已验收环境

| 环境 | 验收范围 |
| --- | --- |
| macOS ARM64、Rust 1.94.0 | 源码安装、独立部署、HTTP 和 Artisan |
| Linux ARM64、Debian Bookworm 容器、Rust 1.94.0 | 以非特权用户运行独立产物、HTTP、SQLite 迁移和 Artisan |
| Linux ARM64、Debian Bookworm 容器、LinuxKit 7.0.14 | 默认同进程启动、SQLite 队列与独立 Artisan 对照、SIGTERM 优雅退出 |
| SQLite 3.51.3 | 干净检出、12 项应用迁移、Artisan 与 HTTP |
| MySQL 5.7.44，Oracle 通用 Linux 归档 | TLS 1.2 证书和服务器名称校验、12 项迁移、幂等重跑、HTTP、数据库会话和 Artisan 作业处理 |
| MySQL 8.0.46、8.4.13 | 隔离数据库、12 项迁移、重复执行、Artisan 数据库 Worker、HTTP 与数据库会话写入 |
| MariaDB 10.3.39、11.8.9 | TLS 校验、12 项迁移、幂等重跑、HTTP、数据库会话、JSON 与 Schema 检查及 Artisan 作业处理 |
| PostgreSQL 10–18 | 隔离数据库、12 项迁移、重复执行、Artisan 数据库 Worker、HTTP 与数据库会话写入。另在 10 和 18 实测任务派发与预留 |
| Redis 8.10 | 队列验收及两个进程间的 Reverb 发布 |
| Ubuntu 24.04 x86_64 与 Apple Silicon 上的 Docker Desktop | Ubuntu 已有原生 amd64 容器验收。当前发布镜像在 Docker Desktop 的 amd64 兼容模式复验 SQLite、Artisan、HTTP、持久存储与 SIGTERM 关停 |
| Parallels 中的 Windows 11 Pro ARM64 Build 26200.9457 | 原生 ARM64 MSVC 发布版通过 SQLite 迁移、Artisan、首页与健康路由、公共文件、目录迁移和 Ctrl+C 优雅退出。Docker Desktop 后端未验收 |

各平台与驱动的验收范围见表，功能边界见相应指南，例如 [Reverb 扩展](../packages/reverb.md)。独立产物的构建和运行见[部署](../getting-started/deployment.md)。

数据库行记录发布验收结果。PostgreSQL 10–18 的结果以实测补丁版本为准。基于 Oracle Linux 7 的 MySQL 5.7.44 容器构建未能与 OpenRust 的 TLS 客户端协商 ECDHE。实测服务端配置见 [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls)，MariaDB 连接配置见 [MariaDB](../database/getting-started.md#mariadb)。预留的 SQL Server 连接当前返回驱动不可用。

当前 Windows ARM64 来宾缺少嵌套虚拟化和可运行的 WSL 2 后端，独立部署已验收。Windows Docker Desktop 和当前版本的原生 Ubuntu 复验仍待目标环境。

本地反向代理通过测试 CA 验证了 TLS 1.2 与 1.3、证书链和主机名。生产端点仍待实机验收。

托管 AWS SQS 标准／FIFO 仍待实机验收。ElasticMQ 的证据见队列实现记录。

## 变更

应用开发者可在仓库的[更新日志](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看变更。迁移已有应用前请查阅[升级指南](upgrade.md)。
