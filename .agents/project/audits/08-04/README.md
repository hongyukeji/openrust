# 08-04 安装、部署和发布资料

以 `34a7a497b9fb56ebe1c213403399b1fe0bad7dac` 的干净检出验收。对照固定 Laravel 的 [安装](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/installation.md)、[部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md)、[发布](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/releases.md)、[升级](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/upgrade.md) 与 [贡献](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/contributions.md)，参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的编译型运行。核对旧版 `6d1a8192` 的 `Dockerfile`、资源打包、`.env.example` 和 Artisan 路径。沿用二进制与资源分开发行和最小权限运行，部署根路径采用当前 standalone 契约，升级文档记录旧环境变量到现行配置的映射。

## 验收

- macOS ARM64、Rust 1.94.0，从干净源码执行锁定构建、环境模板、SQLite 文件创建、`key:generate`、`migrate`、`inspire` 和 `cargo run`。首页及 `/up` 返回 200。
- `cargo +1.94.0 build --release --locked --features standalone` 生成 Mach-O arm64。独立产物从源码目录之外启动，迁移及 Artisan 通过，首页、健康路由、网站和公共磁盘文件可读。移动发布目录后从不同工作目录运行，SIGTERM 正常退出并释放端口。
- 同锁文件和 Rust 1.94.0 在同架构 Linux ARM64 Debian Bookworm 容器生成 ELF aarch64。将产物和资源部署在另一目录，以非特权用户运行 SQLite 迁移、Artisan、HTTP、静态文件和优雅关停。此处为 ARM64 容器验收。
- macOS 独立产物连接独立 MySQL 8.4.13 测试库，四项迁移及 `migrate:status` 通过，测试库和凭据已清理。Redis 8.10 的发布支持记录引用 08-02 的真实双进程验收。
- 干净检出执行贡献指南的格式、锁定工作区测试与严格 Clippy，533 项通过，5 个忽略项由集成测试启动。三语站点构建、12 项通用浏览器检查和 30 项发布相关页面／导航检查通过。
- 本轮后续的 Linux x86_64、Windows 与 TLS 验收范围见下文。现有 v0.x 应用的数据迁移依赖应用自己的 schema、数据格式和迁移历史。本单元更新开发版资料，未创建发布标签。

Docker 开发与部署配套、两个 Linux 镜像架构的 Compose 验收及平台边界见[专项记录](docker.md)。

下一单元 T-01 测试入口与可复用测试应用。

## 托管 AWS SQS 发布验收

使用专用 AWS 标准与 FIFO 队列及独立 MySQL 批次账本，运行真实 Artisan Worker。复验可见性超时、标准队列重复投递、FIFO 去重窗口、并发副本结算，以及发布成功但响应或 SQL 确认失败后的恢复。托管环境的 IAM 与签名权限也随发布验证。核心验收证据使用 ElasticMQ。 [队列验收](../06-03/external-batches.md#并发副本结算)

## 干净检出、Linux amd64 与 Windows ARM64

- 从 `faa57a979692` 全新检出。macOS ARM64 使用 Rust 1.94.0 和隔离构建目录完成锁定构建，复制环境模板、创建 SQLite、执行密钥生成、12 项迁移、`migrate:status` 与 `inspire`。通过 `cargo run` 实测首页和 `/up` 返回 200，SIGTERM 后退出 0、端口关闭。
- Linux amd64 从同一检出构建 release 镜像。Debian 镜像源改用 HTTPS，运行阶段以同 Debian 构建镜像的 CA 证书引导 APT，再安装自身证书包。通过 Docker Desktop 的 Apple Silicon amd64 兼容环境核对 ELF x86_64、UID 10001、运行镜像无 Cargo、SQLite 12 项迁移及幂等重跑、Artisan、HTTP 首页和健康、只读公开文件、公共磁盘、SIGTERM 退出及迁移账本保留。本轮原生 Ubuntu 复验仍待同版本主机，既有原生验收见 [Docker 专项](docker.md#原生-ubuntu-x86_64-实机)。 [Docker 多架构文档](https://docs.docker.com/build/building/multi-platform/)
- Windows 11 Pro ARM64 Build 26200.9457 来宾使用 Rust 1.94.0 与 MSVC 原生锁定 release 构建，PE Machine 为 `0xaa64`。独立发布在含空格的目录中完成 `key:generate`、12 项迁移、重复执行、`migrate:status` 与 `inspire`。从其他工作目录启动，实测首页、健康及两类公共文件；搬移整个发布目录后再次启动并读取原迁移账本。两次真实 Ctrl+C 均退出 0 并释放端口。该来宾未安装 Windows Docker Desktop，Windows 容器后端仍待验收。
- Windows 首轮发现 `key:generate` 锁定文件后第二个句柄读内容触发锁错误。修复后用元数据句柄确认路径当前文件身份，并通过持锁句柄重读，再原子替换环境文件。Windows 复验和 macOS 应用加密回归、目标测试严格 Clippy 通过。

## TLS 与数据库版本矩阵

- 在隔离的本地反向代理用测试 CA 运行生产模式应用，实测 TLS 1.2 与 TLS 1.3 下首页及 `/up`、证书链和主机名验证、错误主机名和未受信任 CA 拒绝、`migrate:status` 与 `inspire`。尚缺生产部署端点及实际部署证书的验收。
- SQLite 3.51.3、隔离 MySQL 8.4.13 与 PostgreSQL 18.6 已通过应用迁移、Artisan 与 HTTP。两种数据库服务均实测 12 项迁移及重复执行无变更。MariaDB 与 SQL Server 实际应用命令返回驱动不可用。矩阵保留各版本的实测范围，其他目标版本需对应环境复验。 [固定 Laravel 数据库范围](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#introduction)
- 当前环境未提供托管 AWS SQS 标准／FIFO 的隔离队列和可用权限。此前 ElasticMQ 验收记录保留，托管 SQS 的 IAM、签名、可见性超时、去重及重试仍待实机测试。

三语站点构建、12 项通用浏览器检查，以及根路径和 `/docs/` 下的 6 项安装、Windows／TLS 部署与发布矩阵导航检查通过。已清理隔离检出、构建产物、临时容器与数据卷、Windows 来宾测试目录、证书与站点。08-04 保持开放。
