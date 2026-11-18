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
- Linux amd64 从同一检出构建 release 镜像。Debian 镜像源改用 HTTPS，运行阶段以同 Debian 构建镜像的 CA 证书引导 APT，再安装自身证书包。通过 Docker Desktop 的 Apple Silicon amd64 兼容环境核对 ELF x86_64、UID 10001、运行镜像无 Cargo、SQLite 12 项迁移及幂等重跑、Artisan、HTTP 首页和健康、只读公开文件、公共磁盘、SIGTERM 退出及迁移账本保留。原生 Ubuntu 后续复验见 [Docker 专项](docker.md#原生-ubuntu-x86_64-发布复验)。 [Docker 多架构文档](https://docs.docker.com/build/building/multi-platform/)
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
- 核对了 [原生 Ubuntu x86_64](docker.md#原生-ubuntu-x86_64-实机) 在 `57bab2532` 的部署记录。该次仅使用 Apple Silicon 宿主及本地 Docker 上下文。原生 x86_64 目标主机的后续复验见 [Docker 专项](docker.md#原生-ubuntu-x86_64-发布复验)。Windows 11 ARM64 来宾实测无 Docker Desktop，`wsl --status` 返回 50，来宾报告未启用虚拟化且无 SLAT。Parallels 的 Windows ARM64 来宾缺少 WSL 2 所需的嵌套虚拟化。核对后恢复来宾原停止状态，Windows Docker Desktop 待目标宿主。 [Docker Desktop 系统要求](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements) · [Parallels Windows ARM64 限制](https://kb.parallels.com/en/129497)
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

## SQL Server TDS 与发布验收

- 从 `73ee5d473` 干净检出。`sqlsrv` 连接使用 Tiberius 与 Rustls，支持 `sqlsrv://` 连接 URL、凭据、连接池和 CA 校验。正文中的 `DB_SSL_CA` 配置与 URL 的 `ssl-ca` 均通过实测。固定基线规定 SQL Server 2017 下限，旧版兼容矩阵有相同下限，未找到可复用的 TDS 客户端。 [固定 Laravel SQL Server 连接器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/SqlServerConnector.php)
- 实测官方 SQL Server 2017 Developer `14.0.3570.2` 与 2022 Developer `16.0.4325.4`。数据库容器在 Apple Silicon 上使用 Linux amd64 兼容运行，应用从宿主经 TCP 连接。隔离服务强制 TLS 1.2，应用查询返回加密状态为 `TRUE`，错误 CA 和错误主机名被拒绝。微软的受支持容器宿主是原生 Linux x86_64，当前版本的原生平台验收仍待目标宿主。 [微软 SQL Server 容器前置条件](https://learn.microsoft.com/en-us/sql/linux/containers/deploy?view=sql-server-2017)
- Rust 实测具名参数、引号与 Unicode、精确定点数、二进制和多结果集、嵌套事务与保存点、自动标识、分页、`MERGE` 与列目录。数据库 Worker 采用 `ROWLOCK`、`UPDLOCK` 和 `READPAST`。真实应用通过 12 项迁移、幂等重跑、回滚及重装、Artisan、首页、`/up`、数据库会话以及 SIGTERM。另在独立数据库运行 HTTP 队列与 Artisan 作业回归。 [固定 Laravel 查询方言](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SqlServerGrammar.php) · [固定 Laravel Schema 方言](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SqlServerGrammar.php)
- 本轮 SQL Server 安装使用预建数据库。其他 SQL Server 版本、Schema 转储与高级方言操作未纳入本次实测。旧 MySQL 镜像的 ECDHE 边界，以及原生 Linux x86_64、Windows Docker Desktop、生产 TLS 和托管 AWS SQS 标准／FIFO 仍在账本中，08-04 保持开放。

工作区严格 Clippy、改动文件格式、三语站点构建、12 项通用浏览器检查和 6 项 SQL Server 支持矩阵导航检查通过。工作区全量格式检查报出干净基线中已有的 `sanctum_authentication.rs` 格式差异。SQL Server 与 MySQL 回归容器已清理，本批新拉取的 SQL Server 镜像标签、隔离源码、构建目录、证书和站点已移除。

## Oracle Linux 7 的 MySQL 5.7.44 ECDHE

- 从 `201519c` 的干净检出锁定构建应用。官方 `mysql:5.7` amd64 镜像的 `openssl-libs` 为 `1.0.2k-26.el7_9`。在强制 TLS 1.2 和同一张 RSA 证书下，原版可协商 DHE，但在 ECDHE 握手中返回失败，Rustls 应用的 Artisan 连接也失败。MySQL 5.7 源码初始化了临时 DH，OpenSSL 1.0.2 提供单独启用自动临时曲线选择的控制项。 [MySQL 固定版本的 TLS 初始化](https://github.com/mysql/mysql-server/blob/mysql-5.7.44/vio/viosslfactories.c) · [OpenSSL 1.0.2k 的 ECDH 接口](https://github.com/openssl/openssl/blob/OpenSSL_1_0_2k/ssl/ssl.h#L1979)
- `docker/mysql57-ecdhe` 从原版摘要构建派生镜像。纯 Rust 共享库仅随 `mysqld` 加载，并在原 OpenSSL 上下文创建后启用 `SSL_CTX_set_ecdh_auto`。库未引入新的动态依赖，原版和派生镜像的 `/usr/sbin/mysqld` SHA-256 相同。运行入口强制安全传输、TLS 1.2 和 ECDHE-RSA AES-GCM 套件，应用沿用 Rustls、CA 和名称校验。
- 以 Apple Silicon 上的 amd64 兼容环境构建并运行派生镜像。独立客户端实测 `ECDHE-RSA-AES128-GCM-SHA256` 与 P-256，证书链及 IP 校验通过。仅允许 DHE 的探针、明文客户端、应用禁用 TLS 的配置、错误 CA 和错误主机名均被拒绝。即使容器参数传入关闭安全传输及 TLS 1.1，实测生效策略仍要求 TLS 1.2 和 ECDHE。
- 派生镜像通过真实 `migrate --force` 的 12 项迁移及幂等重跑、`migrate:status`、`inspire`、首页、`/up`、数据库会话和 SIGTERM。独立数据库通过 HTTP 队列派发、Artisan Worker 作业、重试与失败回归。
- Oracle Linux 7 兼容项按派生镜像验收。原生 Linux x86_64、Windows Docker Desktop、生产 TLS 和托管 AWS SQS 标准／FIFO 继续按目标环境验收，08-04 保持开放。

Rust 共享库通过严格 Clippy、改动文件格式和镜像构建。应用工作区严格 Clippy、三语站点构建、12 项通用浏览器检查和 6 项旧镜像发布矩阵导航检查通过。全量格式检查仍报出基线已有的 `sanctum_authentication.rs` 差异。临时容器、镜像及专用 Buildx 构建器、交叉目标、证书、隔离源码、Cargo 产物和站点已清理。

## 模拟验收补测

以 `4acfcb05b` 的干净归档和 Rust 1.94.0 构建独立应用。08-02 的闭合采用用户确认的 Pusher 协议模拟口径。

- Windows 复核既有 PowerShell／Compose 配置解析、带空格的挂载路径及 Windows ARM64 独立发布的真实 HTTP 和 Artisan。容器内的只读 public、可写 storage 和重建持久化已有 Ubuntu x86_64 实测。Windows Docker Desktop 的文件共享和容器生命周期留给目标后端。 [Docker 专项](docker.md)
- 本地 TLS 既有用例覆盖 TLS 1.2／1.3、证书信任、主机名和生产模式应用。本次新建根 CA、中间 CA 和两张同名称叶证书。客户端仅信任根 CA，代理漏送中间证书时握手失败，补齐链后首页和 `/up` 返回 200。通过同一 HTTPS 监听器换用新证书，客户端核对不同指纹并再次访问成功，首页的 Secure 会话 Cookie 保留。应用的实际 Artisan 迁移和 `migrate:status` 同时通过。
- ElasticMQ 已验证标准队列可见性超时和重投递、FIFO 顺序和去重、溢出载荷、批量逐项失败和 SQL 确认失败恢复。本次用独立回环端点和合成临时凭据重算 AWS JSON 请求的 SigV4，核对区域、服务、请求体及签名中的会话令牌。标准和 FIFO 均通过真实 `queue:retry`、`queue:monitor`、`queue:work`，对应发送、指标、接收与删除。模拟端点对缺令牌或错误签名返回 403，Artisan 退出 1。实际 AWS IAM、STS 会话及服务端语义仍待目标环境。 [既有队列验收](../06-03/sqs.md) · [批量与失败验收](../06-03/external-batches.md) · [AWS SigV4](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html)

## 剩余实机环境复查

- **原生 Linux x86_64** 已复用 Ubuntu 24.04.3 LTS x86_64 主机并验收 `07307c6e1`。完成隔离 Compose 部署、真实 HTTP 和 Artisan、Redis 作业消费、挂载持久化和优雅关停。 [原生 Ubuntu 发布复验](docker.md#原生-ubuntu-x86_64-发布复验)
- **Windows Docker Desktop** 以 `8dc6ac3ca` 为本轮起点，启动现有 Windows 11 ARM64 来宾（Build 26200）复查。系统与用户安装位置均未找到 Docker Desktop，Docker 服务和 CLI 不存在，`wsl --status` 返回 50，固件虚拟化和 SLAT 均报告不可用。两套本地 Docker 上下文均连接 macOS 上的 Linux aarch64 后端。来宾已恢复停止状态。尚缺可登录的 Windows 主机、可用的 Docker Desktop Linux 容器后端，以及测试账号的 Docker、Compose、Buildx 和隔离 Windows 挂载目录读写权限。目标就绪后，从最新提交验证镜像构建、Compose 部署、只读 public、可写且重建后保留的 storage、实际首页与 `/up`、Artisan 迁移和 Worker。 [Docker Desktop 系统要求](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements) · [Parallels 来宾限制](https://kb.parallels.com/en/129497)
- **生产 TLS** 复查确认本地配置未给出生产验收 HTTPS URL。需提供实际 URL 与部署主机名、可受信任的证书链以及同一部署的只读 Artisan 权限。目标就绪后核对 TLS、证书校验、首页与 `/up`，并通过 `migrate:status` 核对运行中的发布实例。现有本地测试 CA 的 TLS 1.2／1.3 记录继续保留。
- **托管 AWS SQS 标准／FIFO** 复查仅发现区域配置，未发现隔离队列 URL 或可用的 AWS 凭据来源。需要标准及 FIFO 各一条隔离队列的完整 URL、对应区域、隔离 MySQL 批次账本和队列范围内的 `SendMessage`、`ReceiveMessage`、`DeleteMessage`、`ChangeMessageVisibility`、`GetQueueAttributes`。批量发送使用 `SendMessage` 权限。临时凭据还需要会话令牌，使用自管 KMS 密钥的队列需向生产者授予 `kms:Decrypt` 和 `kms:GenerateDataKey`，向消费者授予 `kms:Decrypt`。 [AWS SQS API 权限](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html) · [SQS KMS 权限](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-key-management.html)

Windows Docker Desktop、生产 TLS 和托管 AWS SQS 标准／FIFO 的实机验证在目标环境就绪后执行。

三语支持矩阵已同步模拟补测及实机条件。三语站点构建、12 项通用浏览器检查和 6 项矩阵检查通过。模拟服务已退出，隔离源码、构建、证书和站点已清理。

## 08-04 模拟正式环境验收与闭合

2026-11-17 经用户确认，08-04 按模拟正式环境验收闭合。

- 安装、独立发布、数据库版本矩阵与原生 Ubuntu x86_64 部署沿用已验收结果。Ubuntu 复验使用 `07307c6e1`，包含 Compose 部署、持久化挂载、真实 HTTP、Artisan 迁移和独立 Worker。 [原生 Ubuntu 发布复验](docker.md#原生-ubuntu-x86_64-发布复验)
- Windows 采用 ARM64 独立发布的真实 HTTP 和 Artisan、PowerShell 的 Compose 配置和绑定路径验收，以及 Ubuntu 的容器挂载行为实测。生产 TLS 采用本地生产模式应用、测试 CA、证书链校验和轮换验收。托管 SQS 标准／FIFO 采用 ElasticMQ 的投递与失败恢复实测，以及独立端点对真实 Artisan 请求的 SigV4 与会话令牌校验。 [模拟验收补测](#模拟验收补测)
- 真实 Windows Docker Desktop 文件共享与容器生命周期、生产域名及实际证书、AWS IAM／STS 与托管标准／FIFO 服务行为仍列为发布时的实机验证。 [环境与权限](#剩余实机环境复查)

## 主线独立发布包与交付对照复验

- 对照固定 Laravel 部署的公开目录、健康路由和 Worker 生命周期，以及固定 Goravel 的编译型入口。旧版 `openrust-v0.x` 已有 Docker 与独立产物装配，本批沿用当前正式版独立产物流程和既有实际邮件 Worker 验收载荷。 [Laravel 部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md) · [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)
- 以 `2d1f83dfd` 的 `git archive` 源码在 macOS ARM64 上执行 `cargo +1.94.0 build --locked --offline --release --features standalone --bin main`。确认 Mach-O ARM64，锁定元数据唯一解析归档内的 `sqlx-mysql 0.9.0`。运行目录无 Rust 源文件，启动从另一个工作目录执行，无 Cargo 或 `APP_BASE_PATH`。
- 在真实 MySQL 8.4.13 与 Redis 8.10.3 上验证密钥生成、12 项迁移和幂等重跑、待迁移状态、`inspire`、首页、`/up`、网站 `public/` 和存储磁盘访问、数据库会话写入。默认受管 Worker 和独立 `main artisan queue:work redis --once` 分别消费一条实际 Redis 邮件任务，失败表为空。两次 SIGTERM 均退出 0 并释放端口。搬移发布目录后公共存储和迁移账本仍可用。
- Rust 1.94.0 格式检查通过。相同源码的工作区严格 Clippy 已随 [02-04](../02-04/README.md#mysql-和-mariadb-影响行数与整单元验收) 验收。本批三语站点与 `/` 和 `/docs/` 下 12 项通用及 6 项发布小节浏览器检查通过。
- 对照计划与三语文档映射，归档中 104 个本地路径均有三语文件，对照引用的计划锚点齐备。31 行生态映射仍标记范围待确认，文件存在是导航与对应关系的证据，具体功能以交付单元验收为准。 [Laravel 文档对照](../../plans/laravel-docs-delivery-map.md)
- 源码归档和临时服务隔离运行，产物已回收。项目卷净增 2.12 GB（1.97 GiB），余量 100.69 GB（93.78 GiB）。周额度剩余 38%，第二窗口无读数。日志保留于 `/private/tmp/openrust-08-04-current-package.64Mt1XJp`。现有 08-04 模拟验收结论继续有效，实机 Windows Docker Desktop、生产 TLS、托管 SQS、SQL Server 原生 x86_64 和商业嵌入服务各按既有发布检查跟踪。

## 旧版 MySQL 与 MariaDB 驱动复验

- 固定 Laravel 的数据库范围包含 MySQL 5.7 和 MariaDB 10.3。核对 `openrust-v0.x` 的旧兼容范围并复用现行正式版 [MySQL 5.7 ECDHE 派生镜像](#oracle-linux-7-的-mysql-5744-ecdhe)、应用启动入口和 [02-04 影响行数回归](../02-04/README.md#mysql-和-mariadb-影响行数与整单元验收)。 [固定 Laravel 数据库范围](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#introduction) · [MySQL ROW_COUNT](https://dev.mysql.com/doc/refman/5.7/en/information-functions.html#function_row-count) · [MariaDB ROW_COUNT](https://mariadb.com/docs/server/reference/sql-functions/secondary-functions/information-functions/row_count)
- 在 macOS ARM64 用 `d4570522f` 的干净归档和 Rust 1.94.0 离线锁定构建。MySQL 5.7.44 服务使用按摘要构建的 Oracle Linux 7 ECDHE 派生镜像，通过 Docker 的 amd64 兼容后端运行。MariaDB 10.3.39 服务运行于原生 ARM64。两套服务使用隔离临时库和测试证书。
- 两版的 `mysql_family_reports_changed_rows_by_default` 定向测试均通过，覆盖原生 SQL、默认的实际修改行数、连接级匹配行覆盖、upsert 和事务回滚。真实 HTTP 及独立 Artisan 通过 Query Builder 验证原值、实际更新和重复行。会话分别协商 TLS 1.2 和 TLS 1.3。在同端点的控制检查中，证书链验证成功，开启服务器名验证后拒绝不匹配的名称。
- 应用二进制在两版分别通过 `migrate --force` 的 12 项迁移和幂等重跑、`migrate:status --pending=17` 以及 `inspire`。格式检查通过。相同代码的严格 Clippy 已随 02-04 通过。三语站点、12 项通用浏览器检查及两种部署路径下的 6 项发布到 Query Builder 文档链路通过。
- 本批核实旧版 MySQL 与 MariaDB 的握手和受影响写入行为。临时数据库、证书、专用构建器、临时镜像及隔离产物已回收，日志保留于 `/private/tmp/openrust-08-04-found-rows-minimum.hg0TaIjL`。回收编译和文档产物时项目卷净增 4.86 GB（4.53 GiB），剩余 96.67 GB（90.03 GiB）。周额度剩余 38%，第二窗口无读数。

## 当前主线整仓回归与独立发布复验

- 以 `da0b98aaa` 的干净归档为发布基线，对照固定 Laravel 的健康路由、部署目录和服务生命周期，以及固定 Goravel 的编译型入口。核对 `openrust-v0.x` 的 `.agents/project/scripts/test-artisan-release.sh`、`tools/cli/src/release.rs`，复用正式版现有单二进制发布及 Worker 实际请求夹具。 [固定 Laravel 部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md) · [固定 Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)
- 在隔离 MySQL 8.4.13 和两个 Redis 8.10.3 实例上执行 Rust 1.94.0 `cargo test --workspace --all-targets --locked --offline --no-fail-fast -- --test-threads=1`。首次共三个测试目标失败。批次测试的大 Future 移到堆上并以默认测试栈复验。大型多态 Seeder 夹具只提高合成测试子进程的栈额度，恢复后同库的后续用例不再遇到残留表冲突。表单验证 fixture 为每个 HTTP 应用创建专用 MySQL Session 表，结束后清理，另复验 Array 存储。最终合并原整仓中未修改目标与三个修正目标的独立重跑证据，共 168 个目标，1340 项标准测试通过、9 项忽略，两个自定义测试程序通过。Rust 1.94.0 全仓格式与严格 Clippy 通过。
- 从同一快照锁定离线构建 `--release --features standalone --bin main`，确认 Mach-O ARM64。在无源码目录用独立二进制运行实际 `key:generate`、12 项迁移与幂等重跑、`migrate:status`、`inspire` 和 `schedule:list --json`。真实首页、`/up`、公共目录与存储磁盘、MySQL 会话，受管及独立 Redis 邮件 Worker 都通过。移到带空格的不同路径后，迁移账本与存储文件保持可用，两次 SIGTERM 均退出 0。
- 发布说明同步三语健康检查示例。三语站点及 18 项浏览器检查通过。104 个本地映射路径在三种语言均存在，计划锚点齐全。现有 31 条生态条目保持交付计划的待确认状态，08-04 继续沿用已确认的模拟验收边界。临时容器、发布目录及隔离构建、文档产物已回收。提交前磁盘可用 98.52 GB（91.75 GiB），周额度剩余 35%。日志保留于 `/private/tmp/openrust-current-release-check.5q9G5KN4`。

## HTTPS 反向代理下的配置缓存与签名链接

- 2026-11-18 在 `0db6bac21` 的隔离源码上，以生产模式启动真实应用。测试代理在回环地址终止 TLS，并将 HTTPS 协议、入口域名和端口转发给仅信任回环代理的后端。客户端分别用 TLS 1.2 和 TLS 1.3 连接，验证测试 CA 和主机名。
- 真实 Artisan `config:cache` 和 `config:show` 确认缓存中的 `app.url` 为对外 HTTPS 地址且 `app.url_explicit=true`。修改测试环境文件后，应用仍使用缓存中的地址。通过另一个入口域名请求时，普通链接和绝对签名链接均指向 `APP_URL`。
- 两种 TLS 版本下，入口域名的 `/up` 和通过 `APP_URL` 域名访问的签名链接均返回 HTTP 200。同一签名链接换用入口域名或修改路径时返回 HTTP 403。应用和代理已停止，隔离源码、测试证书、构建产物和测试缓存已清理。实际生产域名与证书继续按发布实机项跟踪。

## 独立发布的配置缓存与 Artisan 帮助

- 对照固定 Laravel 部署的配置缓存与健康路由，以及固定 Goravel 的编译入口。复核 `openrust-v0.x` 的 `.agents/project/scripts/test-artisan-release.sh`，沿用当前主线已有的发布目录、真实 MySQL／Redis 与 Worker 验收流程。 [Laravel 部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md#optimizing-configuration-loading) · [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)
- 从 `7fadb3793` 的 Git 归档用 Rust 1.94.0 执行 `cargo build --release --features standalone --bin main --locked --offline`，产物为 macOS ARM64 Mach-O。在无源码、无 Cargo、无 `APP_BASE_PATH` 的发布目录，从其他工作目录启动。用同一二进制验证英、简中、繁中完整纯文本帮助、ANSI 与 JSON 内容。
- 真实 MySQL 8.4.13 和 Redis 8.10.3 上，独立 Artisan 生成配置缓存后，HTTP 与 `config:show` 都采用快照。清除后读取新的进程环境。搬移带空格的发布目录后重新生成，缓存内的本机磁盘根路径指向新位置，首页和公共存储仍正常。12 项迁移及幂等重跑、受管与独立 Redis Worker、数据库 Session、`/up`、两次 SIGTERM 退出 0 与端口回收均通过。
- 三语部署页补充配置缓存步骤和搬移后的重建方式。全仓格式通过，同一份框架源码的严格 Clippy 见 [06-08](../06-08/README.md#artisan-完整纯文本帮助)。三语构建和根路径及 `/docs/` 下的 18 项浏览器检查通过。项目仍按已确认的发布模拟口径记录，Windows Docker Desktop、生产 TLS 和托管 SQS 的目标环境待实机。
- 临时 Redis、MySQL、构建和测试发布目录已回收，Docker 数据卷清单一致。磁盘可用 88.99 GB（82.88 GiB），提交前周额度剩余 27%，第二窗口无读数。日志保留于 `/private/tmp/openrust-release-artisan-main.Ij3lTvEs`。

## Artisan 服务重载

- 对照固定 Laravel v13.40.0 的 `ReloadCommand` 与 `ServiceProvider::reloads()`。旧版 `openrust-v0.x` 未提供统一重载的对应实现。OpenRust 复用已有 `queue:restart`、`schedule:interrupt` 和独立 `reverb:restart` 命令。Provider 的任务表按应用存放，使用当前 Artisan 注册表安静调用命令。保留固定 Laravel 的任务顺序、按名称或命令排除、任务返回非零时标记失败并继续，以及整体退出 0 的语义。 [固定 Laravel ReloadCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ReloadCommand.php) · [固定 Laravel ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php#L480)
- 隔离快照 `c7177e8f6` 的 Rust 1.94.0 启用 `reverb` 功能构建。独立 Artisan 在真实 MySQL 8.4.13 和 Redis 8.10.3 中完成迁移及一项邮件作业。按标签和按命令排除后 Worker 继续运行，执行完整重载后优雅退出 0，Redis 显示三个重载目标的信号。独立调度进程在真实 Redis 收到重载的中断信号后停止重复任务。Reverb 通过实际 HTTP 和 WebSocket 复验，独立实例被排除时保持运行，收到重载后关闭连接并退出 0。
- 20 项 `console`、`feature`、`artisan_entry`、Redis 调度跨进程回归、全仓格式和启用 Reverb 的工作区全目标严格 Clippy 通过。三语部署文档补充命令、共享缓存和统一主进程边界。在根路径及 `/docs/` 下的三语站点构建与 18 项浏览器检查通过。临时容器、编译及文档产物已回收。磁盘可用 88.95 GB（82.84 GiB），提交前周额度剩余 26%，第二窗口无读数。日志保留于 `/private/tmp/openrust-artisan-reload.3gsZBpZR`。

## Artisan 部署优化与缓存清理

- 对照固定 Laravel v13.40.0 的 `OptimizeCommand`、`OptimizeClearCommand` 与 `ServiceProvider::optimizes()`。复核 `openrust-v0.x` 的编译 Provider 清单及 `clear-compiled`，复用当前正式版子命令和 `reload` 已验收的任务组织。支持按任务名或命令名排除、按应用注册 Provider 扩展，以及继续处理返回非零状态的任务。选择的 `--env` 继续由同一个 Artisan 命令定义解析并传给任务。当前持久化路由缓存仍待实现，路由继续在编译注册基础上启动装配。 [固定 Laravel OptimizeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/OptimizeCommand.php) · [固定 Laravel OptimizeClearCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/OptimizeClearCommand.php)
- 在隔离快照 `6b525df0e` 用实际应用二进制生成配置、事件发现和编译视图产物。实测 `--env=staging`、排除事件任务、HTTP 首页及 `/up`、包清单移除、清理后恢复读取默认环境。真实 Redis 8.10.3 验证排除 `cache:clear` 保留测试键，完整清理删除该键并支持幂等重跑。重跑真实 MySQL 8.4.13／Redis 的 Worker、跨进程调度和独立 Reverb HTTP／WebSocket，复验原有重载委派与排除行为。
- 21 项 `console`、`feature`、`artisan_entry`、Rust 1.94.0 全仓格式、启用 Reverb 的工作区全目标严格 Clippy 通过。三语站点及根路径与 `/docs/` 下 18 项浏览器检查通过。临时 Redis、编译及站点产物已回收。磁盘可用 88.93 GB（82.82 GiB），提交前周额度剩余 26%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-artisan-optimize.LOtOxdzV`。

部署优化的持久化路由缓存已接通，`optimize` 和 `optimize:clear` 分别生成与删除索引。 [路由缓存验收](../04-01/README.md#持久化路由匹配索引)

## 独立部署的缓存路径与包清单清理

- 对照固定 Laravel v13.40.0 的 `getCachedPackagesPath()`、`clear-compiled` 和 `optimize:clear`。复核 openrust-v0.x 的服务与包清单清理夹具，沿用缓存间相互独立的验收思路。OpenRust 的延迟 Provider 元数据已随二进制链接，`clear-compiled` 清除按需导出的包清单。 [Laravel Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php) · [Laravel ClearCompiledCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ClearCompiledCommand.php)
- 在隔离主线 `80e98bc87` 以 Rust 1.94.0 构建启用 `standalone` 的 macOS ARM64 Debug 二进制。把二进制移到带空格且无源码的目录，从另一个工作目录运行。实测冷启动 `inspire`、独立导出包清单、`optimize`、`config:show`、`route:list`、`event:list` 和两次 `optimize:clear --except=cache`。分别覆盖绝对和相对路径、设置快照中的视图编译目录、清理同目录时保留无关文件。使用 Array 存储。配置缓存生效时实际首页、`/up` 和公共静态文件返回 200，SIGTERM 退出 0。清除后 Artisan 读取更新的 dotenv 值。
- 38 项定向回归、全仓格式、启用 `standalone` 的工作区全目标严格 Clippy、三语站点构建和 18 项浏览器检查通过。回收本批生成文件，磁盘可用 86.64 GB（80.69 GiB）。提交前周额度剩余 24%，第二窗口无读数，按用户授权继续。记录保留于 `/private/tmp/openrust-release-cache-paths.0eh1TIO3`。
