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

## 数据库版本复验与平台前置条件

- 以 `a1b12f149` 的干净检出和本批数据库队列修复构建独立应用。固定 Laravel 文档的数据库下限为 MariaDB 10.3、MySQL 5.7、PostgreSQL 10、SQLite 3.26 和 SQL Server 2017。OpenRust 默认使用随二进制编译的 SQLite，服务端数据库按实测版本记录。 [固定 Laravel 数据库范围](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#introduction)
- SQLite 3.51.3、MySQL 8.0.46／8.4.13、PostgreSQL 10.21／11.16／12.22／13.23／14.25／15.20／16.16／17.12／18.6 通过 12 项迁移、幂等重跑、`migrate:status`、`inspire`、独立 `queue:work`、首页和 `/up`，并验证进程收到 SIGTERM 后退出为 0。MySQL、PostgreSQL 服务使用本地 ARM64 容器和隔离数据库，另核对了应用写入数据库会话。
- PostgreSQL 迁移成功后，默认数据库 Worker 曾因使用 MySQL 的时钟函数退出。改用 PostgreSQL 语句时间的 Unix 秒值，入队使用 `RETURNING id`，继续使用基线的 `FOR UPDATE SKIP LOCKED`。真实 HTTP 回归在 PostgreSQL 10.21、18.6、SQLite 和 MySQL 8.4.13 上验证了派发、预留、延迟、释放、删除和事务，队列测试严格 Clippy 通过。 [固定 Laravel 数据库队列](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/DatabaseQueue.php#L489)
- 官方 MySQL 5.7.44 镜像在本地 amd64 兼容环境中启动，容器内客户端连接成功，应用的必需 TLS 握手收到 `HandshakeFailure`。服务端限定测试的 ECDHE 套件时连容器内客户端也未握手成功。该版本仍待验证，MariaDB 与 SQL Server 仍归未接入的驱动范围。
- 核对了 [原生 Ubuntu x86_64](docker.md#原生-ubuntu-x86_64-实机) 在 `57bab2532` 的部署记录。当前工作区只有 Apple Silicon 宿主及本地 Docker 上下文，当前版本仍待原生 x86_64 目标主机复验。Windows 11 ARM64 来宾实测无 Docker Desktop，`wsl --status` 返回 50，来宾报告未启用虚拟化且无 SLAT。Parallels 的 Windows ARM64 来宾缺少 WSL 2 所需的嵌套虚拟化。核对后恢复来宾原停止状态，Windows Docker Desktop 待目标宿主。 [Docker Desktop 系统要求](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements) · [Parallels Windows ARM64 限制](https://kb.parallels.com/en/129497)
- 按约定保留生产 TLS 和托管 AWS SQS 标准／FIFO 的实机验收，08-04 保持开放。

三语站点构建、12 项通用浏览器检查和 6 项发布矩阵导航检查通过。已清理本批隔离源码、Cargo 产物、站点、数据库容器以及新拉取的数据库镜像标签。

## MySQL 5.7 TLS 与驱动边界

- 从 `22acc2558` 的干净检出锁定构建。隔离运行官方 `mysql:5.7` 的 5.7.44 Oracle Linux 7 构建，确认它动态链接 OpenSSL 1.0.2k。RSA 证书的独立客户端协商 `DHE-RSA-AES128-GCM-SHA256` 成功，使用相同证书测试 ECDHE 时收到握手失败，Rust 应用也拒绝该端点。单独换用 ECDSA 证书仍未接通 ECDHE。 [MySQL 5.7 TLS 库说明](https://dev.mysql.com/doc/mysql-security-excerpt/5.7/en/ssl-libraries.html)
- 在同一 amd64 兼容环境使用 Oracle `mysql-5.7.44-linux-glibc2.12-x86_64.tar.gz` 的实际 `mysqld`。服务端强制安全传输，限定 TLS 1.2 和 ECDHE-RSA AES-GCM，独立探针协商 `ECDHE-RSA-AES128-GCM-SHA256`，证书链及地址校验通过。应用设置 `MYSQL_ATTR_SSL_CA` 后通过连接，明文连接、应用禁用 TLS 的配置、错误 CA 和独立客户端校验错误主机名均被拒绝。 [Oracle MySQL 归档](https://downloads.mysql.com/archives/community/)
- 真实应用通过 12 项迁移、幂等重跑、`migrate:status`、`inspire`、`queue:work`、首页和 `/up`、数据库会话写入与 SIGTERM。固定 Laravel 在 MySQL 8.0.1 起使用 `SKIP LOCKED`，旧版本回退到行锁。Rust 通过 MySQL 版本注释让当前事务连接上的服务器选择语句，两种 MySQL 版本分别通过真实 HTTP 队列及 Artisan 作业执行、重试和失败回归，复测版本为 5.7.44 和 8.4.13。队列测试严格 Clippy 通过。 [固定 Laravel 的队列锁选择](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/DatabaseQueue.php#L489)
- `mariadb` 和 `sqlsrv` 分别使用真实 `artisan migrate:status`，均以 1 退出并报告驱动不可用。应用配置已预留两者，连接工厂的物理连接池和 SQL 方言目前仅接通 SQLite、MySQL 及 PostgreSQL。MariaDB 仍需独立接通连接名、版本能力和 Schema 方言。SQL Server 仍缺 TDS 连接与 TLS、参数绑定和 SQL／Schema 方言。两者都需要真实服务的连接、迁移、HTTP 和 Worker 验收。
- 原生 Linux x86_64、Windows Docker Desktop、生产 TLS 与托管 AWS SQS 标准／FIFO 保留到环境就绪后验收。08-04 保持开放。

三语站点构建、12 项通用浏览器检查和 6 项 MySQL 5.7 发布矩阵与 TLS 配置导航检查通过。已清理隔离数据库、镜像标签、MySQL 归档、证书、站点与构建产物。

## MariaDB 连接与方言

- `mariadb` 连接与 `mariadb://` URL 通过 SQLx 的 MySQL 协议池和 Rustls 接通。沿用 MySQL TLS 模式及证书校验、角色连接与严格 SQL 模式，MariaDB 使用独立的方言选择。旧版 `6d1a8192` 的兼容矩阵列出 MariaDB 10.3 下限，未提供可复用的 MariaDB 专用驱动。 [固定 Laravel MariaDB 连接器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/MariaDbConnector.php)
- 按基线实现生成 JSON 列的 `json_value`、JSON 路径更新、拒绝 lateral join、10.7 起原生 UUID、10.5.2 前读取原列定义后重命名，以及 10.6 起跳过被锁定的队列行。两端实测 JSON 字符串、布尔、对象更新、旧默认值与排序规则保留、UUID 类型及空间列声明。 [固定 Laravel 查询方言](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/MariaDbGrammar.php) · [固定 Laravel Schema 方言](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MariaDbGrammar.php)
- 两个 ARM64 隔离容器分别运行 MariaDB 10.3.39 和 11.8.9。真实应用均通过具名和 URL 连接、12 项迁移及幂等重跑、Artisan、首页和 `/up`、数据库会话写入和 SIGTERM。测试账号要求 TLS，拒绝明文、错误 CA 和错误主机名。数据库队列的 HTTP 派发与独立 Artisan Worker 作业、重试和失败回归在两版均通过。

严格 Clippy、三语站点构建、12 项通用浏览器检查和 6 项发布矩阵导航检查通过。MariaDB 容器、镜像标签、证书、隔离站点和构建目录已清理。
