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
| MySQL 5.7.44，Oracle Linux 7 派生镜像 | 在 amd64 兼容环境通过 TLS 1.2 和 ECDHE 校验、12 项迁移、幂等重跑、HTTP、数据库会话及 Artisan 作业处理 |
| MySQL 8.0.46、8.4.13 | 隔离数据库、12 项迁移、重复执行、Artisan 数据库 Worker、HTTP 与数据库会话写入 |
| MariaDB 10.3.39、11.8.9 | TLS 校验、12 项迁移、幂等重跑、HTTP、数据库会话、JSON 与 Schema 检查及 Artisan 作业处理 |
| SQL Server 2017（14.0.3570.2）、2022（16.0.4325.4） | 对 Linux amd64 兼容环境中的服务端容器实测 Rust TDS 和 TLS 校验、12 项迁移、幂等重跑、HTTP、数据库会话及 Artisan 作业处理 |
| PostgreSQL 10–18 | 隔离数据库、12 项迁移、重复执行、Artisan 数据库 Worker、HTTP 与数据库会话写入。另在 10 和 18 实测任务派发与预留 |
| Redis 8.10 | 队列验收及两个进程间的 Reverb 发布 |
| Ubuntu 24.04.3 LTS x86_64、Docker Engine 29.9.3 | 从 `07307c6e1` 使用 Rust 1.94.0 原生构建发布镜像，实测 Compose 的 MySQL 与 Redis、12 项迁移、HTTP 和 Artisan、Redis 作业、双应用实例共享数据库与缓存、存储持久化及 SIGTERM 关停 |
| Apple Silicon 上的 Docker Desktop、amd64 兼容模式 | SQLite、Artisan、HTTP、持久存储与 SIGTERM 关停 |
| Parallels 中的 Windows 11 Pro ARM64 Build 26200.9457 | 原生 ARM64 MSVC 发布版通过 SQLite 迁移、Artisan、首页与健康路由、公共文件、目录迁移和 Ctrl+C 优雅退出。Docker Desktop 后端未验收 |

各平台与驱动的验收范围见表，功能边界见相应指南，例如 [Reverb 扩展](../packages/reverb.md)。独立产物的构建和运行见[部署](../getting-started/deployment.md)。

数据库行记录发布验收结果。PostgreSQL 10–18 的结果以实测补丁版本为准。Oracle Linux 7 原镜像需构建[启用 ECDHE 的派生镜像](../database/getting-started.md#mysql-57-oracle-linux-7)以接入 OpenRust。实测连接配置见 [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls)、[MariaDB](../database/getting-started.md#mariadb) 和 [SQL Server](../database/getting-started.md#sql-server)。

## 待实机验收 {#pending-deployment-acceptance}

| 目标 | 所需环境或权限 |
| --- | --- |
| Windows Docker Desktop | 可登录的 Windows 主机，Docker Desktop 正在运行 Linux 容器后端。测试账号须能执行 `docker info`、`docker compose version` 和 `docker buildx version`，并能读写用于绑定挂载的隔离 Windows 测试目录。现有 Parallels ARM64 来宾没有 Docker Desktop 和可用的 WSL 2 后端。 |
| 生产 TLS | 实际 HTTPS URL、部署主机名、可受信任的证书链，以及在同一部署执行只读 Artisan 命令的权限。本地反向代理已用测试 CA 验证 TLS 1.2 和 1.3。 |
| 托管 AWS SQS 标准／FIFO | 两条隔离队列的 URL（标准与 FIFO 各一）、AWS 区域、隔离 MySQL 批次账本，以及仅授予测试队列权限的凭据。核心检查需要 `sqs:SendMessage`（包含批量发送）、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes`。使用临时凭据时需同时配置会话令牌。 |

队列权限见 [AWS SQS API 参考](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html)。

## 变更

应用开发者可在仓库的[更新日志](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看变更。迁移已有应用前请查阅[升级指南](upgrade.md)。
