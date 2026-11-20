# 开发者文档边界整理：docs/zh_CN/prologue/releases.md

2026-11-20。历史原文对照，不作为当前验收结论。

````diff
--- docs/zh_CN/prologue/releases.md (迁移前)
+++ docs/zh_CN/prologue/releases.md (开发者正文)
@@ -4,50 +4,17 @@

 当前源码在 `Cargo.toml` 中声明开发版 `0.0.0`。构建时使用已提交的 `Cargo.lock`，部署时记录源码提交号。OpenRust 版本号描述 OpenRust 自身的版本。Laravel v13.40.0 是兼容基线，应用启动与装配参考 Goravel v1.18.0。 [Laravel 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 源码](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

-## 已验收环境
+## 环境兼容范围

-| 环境 | 验收范围 |
-| --- | --- |
-| macOS ARM64、Rust 1.94.0 | 源码安装、独立部署、HTTP 和 Artisan |
-| Linux ARM64、Debian Bookworm 容器、Rust 1.94.0 | 以非特权用户运行独立产物、HTTP、SQLite 迁移和 Artisan |
-| Linux ARM64、Debian Bookworm 容器、LinuxKit 7.0.14 | 默认同进程启动、SQLite 队列与独立 Artisan 对照、SIGTERM 优雅退出 |
-| SQLite 3.51.3 | 干净检出、12 项应用迁移、Artisan 与 HTTP |
-| MySQL 5.7.44，Oracle 通用 Linux 归档 | TLS 1.2 证书和服务器名称校验、12 项迁移、幂等重跑、HTTP、数据库会话和 Artisan 作业处理 |
-| MySQL 5.7.44，Oracle Linux 7 派生镜像 | 在 amd64 兼容环境通过 TLS 1.2 和 ECDHE 校验、12 项迁移、幂等重跑、HTTP、数据库会话及 Artisan 作业处理 |
-| MySQL 8.0.46、8.4.13 | 隔离数据库、12 项迁移、重复执行、Artisan 数据库 Worker、HTTP 与数据库会话写入 |
-| MariaDB 10.3.39、11.8.9 | TLS 校验、12 项迁移、幂等重跑、HTTP、数据库会话、JSON 与 Schema 检查及 Artisan 作业处理 |
-| SQL Server 2017（14.0.3570.2）、2022（16.0.4325.4） | 对 Linux amd64 兼容环境中的服务端容器实测 Rust TDS 和 TLS 校验、12 项迁移、幂等重跑、HTTP、数据库会话及 Artisan 作业处理 |
-| PostgreSQL 10–18 | 隔离数据库、12 项迁移、重复执行、Artisan 数据库 Worker、HTTP 与数据库会话写入。另在 10 和 18 实测任务派发与预留 |
-| Redis 8.10 | 队列验收及两个进程间的 Reverb 发布 |
-| Ubuntu 24.04.3 LTS x86_64、Docker Engine 29.9.3 | 从 `07307c6e1` 使用 Rust 1.94.0 原生构建发布镜像，实测 Compose 的 MySQL 与 Redis、12 项迁移、HTTP 和 Artisan、Redis 作业、双应用实例共享数据库与缓存、存储持久化及 SIGTERM 关停 |
-| Apple Silicon 上的 Docker Desktop、amd64 兼容模式 | SQLite、Artisan、HTTP、持久存储与 SIGTERM 关停 |
-| Parallels 中的 Windows 11 Pro ARM64 Build 26200.9457 | 原生 ARM64 MSVC 发布版通过 SQLite 迁移、Artisan、首页与健康路由、公共文件、目录迁移和 Ctrl+C 优雅退出。Docker Desktop 后端未验收 |
+源码已在 macOS ARM64、Linux ARM64／x86_64 和 Windows ARM64 上运行验证。须针对目标操作系统与架构构建，二进制不能跨平台通用。Windows Docker Desktop 需要可用的 Linux 容器后端。

-各平台与驱动的验收范围见表，功能边界见相应指南，例如 [Reverb 扩展](../packages/reverb.md)。独立产物的构建和运行见[部署](../getting-started/deployment.md)。
+现有驱动检查覆盖 SQLite 3.51.3、MySQL 5.7.44／8.0.46／8.4.13、MariaDB 10.3.39／11.8.9、SQL Server 2017／2022 和 PostgreSQL 10–18。这仅描述特定版本和场景的检查范围，不代表所有平台、补丁版本和功能组合均可用。驱动配置及版本限制见[数据库](../database/getting-started.md)。Redis 队列和 Reverb 集成已在 Redis 8.10 上运行验证。

-数据库行记录发布验收结果。PostgreSQL 10–18 的结果以实测补丁版本为准。Oracle Linux 7 原镜像需构建[启用 ECDHE 的派生镜像](../database/getting-started.md#mysql-57-oracle-linux-7)以接入 OpenRust。实测连接配置见 [MySQL 5.7 TLS](../database/getting-started.md#mysql-57-tls)、[MariaDB](../database/getting-started.md#mariadb) 和 [SQL Server](../database/getting-started.md#sql-server)。
+## 部署要求

-## 独立发布包复验 {#standalone-package-recheck}
+按[部署指南](../getting-started/deployment.md)构建和运行独立产物。MySQL 5.7 须配置[支持 ECDHE 的 TLS](../database/getting-started.md#mysql-57-tls)；Oracle Linux 7 镜像需使用[派生镜像](../database/getting-started.md#mysql-57-oracle-linux-7)。移植数据库写入逻辑时，参阅[影响行数](../database/queries.md#mysql-affected-rows)。

-从 `2d1f83dfd` 的干净源码归档在 macOS ARM64 上用 Rust 1.94.0 和提交的锁文件构建独立发布二进制。发布目录从其他工作目录启动，连接 MySQL 8.4.13 和 Redis 8.10.3。密钥生成、12 项迁移与幂等重跑、首页和健康路由、两类公共文件、数据库会话、受管和独立 Redis Worker，以及 SIGTERM 优雅退出均通过。搬移发布目录后仍可访问公共存储和原迁移账本。
-
-## 旧版 MySQL 与 MariaDB 驱动复验 {#older-mysql-family-recheck}
-
-Oracle Linux 7 ECDHE 派生镜像中的 MySQL 5.7.44 和 MariaDB 10.3.39 分别通过 TLS 1.2 和 TLS 1.3 校验及影响行数回归。HTTP 和 Artisan 都验证了原值更新和重复行 upsert，两个版本均通过应用迁移与幂等重跑。MySQL 服务端运行于 amd64 兼容环境，MariaDB 服务端运行于 ARM64。默认行为及连接覆盖见[影响行数](../database/queries.md#mysql-affected-rows)。
-
-## 模拟正式环境与实机验证 {#pending-deployment-acceptance}
-
-08-04 已按模拟正式环境验收闭合。表中列有各目标环境的实机验证项。
-
-| 目标 | 已有本地验收 | 实机验证 |
-| --- | --- | --- |
-| Windows Docker Desktop | Windows PowerShell 已解析 Compose profile 和含空格的绑定路径。Windows ARM64 独立发布已通过 HTTP 和 Artisan。 | 需要可登录且运行 Docker Desktop Linux 容器后端的 Windows 主机，测试账号可使用 Docker、Compose 和 Buildx，并读写隔离的 Windows 测试目录。实测干净构建、部署、只读 public、可写持久化 storage、HTTP 和 Artisan。 |
-| 生产 TLS | 本地代理验证了 TLS 1.2／1.3、主机名与 CA 校验、缺少中间证书时拒绝、同一端点轮换叶证书、真实 HTTP 与 Secure 会话 Cookie。 | 在真实 HTTPS 地址核对部署主机名、证书链和协议策略，实测首页和健康路由，并对同一部署运行只读 Artisan。 |
-| 托管 AWS SQS 标准／FIFO | ElasticMQ 已验证投递语义和批次恢复。独立回环端点重算两种队列的真实 Artisan 发送、指标、接收及删除请求的 SigV4，并拒绝缺少会话令牌和无效签名。 | 需要标准与 FIFO 各一条隔离队列的 URL、对应区域、队列范围 IAM 凭据和隔离 MySQL 批次账本。用真实 Worker 验证 AWS 侧 IAM 与临时凭据、队列指标、重投递、FIFO 去重和失败恢复。 |
-
-测试凭据须限制在测试队列，具备 `sqs:SendMessage`（含批量发送）、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 权限。使用临时凭据时须同时配置会话令牌。
-
-队列权限见 [AWS SQS API 参考](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html)。
+生产 TLS 由反向代理或负载均衡器终止，须核对部署主机名和证书链。托管 SQS 需要正确的区域、队列 URL 和 IAM 凭据，本地协议测试不能确认 AWS 侧权限。凭据按队列授予 `sqs:SendMessage`、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 权限，临时凭据同时配置会话令牌。参阅[队列](../digging-deeper/queues.md)。

 ## 变更

````
