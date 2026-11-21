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

- 对照固定 Laravel v13.40.0 的 `OptimizeCommand`、`OptimizeClearCommand` 与 `ServiceProvider::optimizes()`。复核 `openrust-v0.x` 的编译 Provider 清单及 `clear-compiled`，复用当前正式版子命令和 `reload` 已验收的任务组织。支持按任务名或命令名排除、按应用注册 Provider 扩展，以及继续处理返回非零状态的任务。选择的 `--env` 继续由同一个 Artisan 命令定义解析并传给任务。该次快照尚无持久化路由缓存；后续已接通，当前实现见[路由缓存验收](../04-01/README.md#持久化路由匹配索引)。 [固定 Laravel OptimizeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/OptimizeCommand.php) · [固定 Laravel OptimizeClearCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/OptimizeClearCommand.php)
- 在隔离快照 `6b525df0e` 用实际应用二进制生成配置、事件发现和编译视图产物。实测 `--env=staging`、排除事件任务、HTTP 首页及 `/up`、包清单移除、清理后恢复读取默认环境。真实 Redis 8.10.3 验证排除 `cache:clear` 保留测试键，完整清理删除该键并支持幂等重跑。重跑真实 MySQL 8.4.13／Redis 的 Worker、跨进程调度和独立 Reverb HTTP／WebSocket，复验原有重载委派与排除行为。
- 21 项 `console`、`feature`、`artisan_entry`、Rust 1.94.0 全仓格式、启用 Reverb 的工作区全目标严格 Clippy 通过。三语站点及根路径与 `/docs/` 下 18 项浏览器检查通过。临时 Redis、编译及站点产物已回收。磁盘可用 88.93 GB（82.82 GiB），提交前周额度剩余 26%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-artisan-optimize.LOtOxdzV`。

部署优化的持久化路由缓存已接通，`optimize` 和 `optimize:clear` 分别生成与删除索引。 [路由缓存验收](../04-01/README.md#持久化路由匹配索引)

## 独立部署的缓存路径与包清单清理

- 对照固定 Laravel v13.40.0 的 `getCachedPackagesPath()`、`clear-compiled` 和 `optimize:clear`。复核 openrust-v0.x 的服务与包清单清理夹具，沿用缓存间相互独立的验收思路。OpenRust 的延迟 Provider 元数据已随二进制链接，`clear-compiled` 清除按需导出的包清单。 [Laravel Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php) · [Laravel ClearCompiledCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ClearCompiledCommand.php)
- 在隔离主线 `80e98bc87` 以 Rust 1.94.0 构建启用 `standalone` 的 macOS ARM64 Debug 二进制。把二进制移到带空格且无源码的目录，从另一个工作目录运行。实测冷启动 `inspire`、独立导出包清单、`optimize`、`config:show`、`route:list`、`event:list` 和两次 `optimize:clear --except=cache`。分别覆盖绝对和相对路径、设置快照中的视图编译目录、清理同目录时保留无关文件。使用 Array 存储。配置缓存生效时实际首页、`/up` 和公共静态文件返回 200，SIGTERM 退出 0。清除后 Artisan 读取更新的 dotenv 值。
- 38 项定向回归、全仓格式、启用 `standalone` 的工作区全目标严格 Clippy、三语站点构建和 18 项浏览器检查通过。回收本批生成文件，磁盘可用 86.64 GB（80.69 GiB）。提交前周额度剩余 24%，第二窗口无读数，按用户授权继续。记录保留于 `/private/tmp/openrust-release-cache-paths.0eh1TIO3`。

## HTTP 章节与 2702d577b 发布快照复验

- 对照固定 Laravel 的客户端 IP、普通 JSON 响应、健康路由和 Goravel 编译入口。旧版 Artisan 发布脚本及正式版独立发布夹具提供可复用的验收流程。三语文档单列请求 IP 链与普通 JSON 响应，固定 Laravel PHP 8.5 夹具复验可信地址排除、全部可信回退、忽略非可信转发和普通 JSON 状态及响应头。 [Laravel Requests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#request-ip-address) · [Laravel Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#json-responses) · [Laravel 部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md#the-health-route)
- 从 `2702d577b` 干净归档，在隔离 MySQL 8.4.13 与真实 Redis 上以 Rust 1.94.0 执行 `cargo test --workspace --all-targets --locked --offline --no-fail-fast -- --test-threads=1`。整仓 171 个测试目标，1397 项标准测试通过、11 项忽略；实际独立发布通过。全仓格式和严格 Clippy 通过。并行合入的 SMS 提交 `36e7b304a` 发生在取快照之后，其独立验收见 [短信渠道记录](../06-06/sms-providers.md)。
- 同源快照构建 `--release --features standalone --bin main --locked --offline`，产物为 macOS ARM64 Mach-O。从无源码目录运行真实 HTTP、Artisan、MySQL 迁移及幂等重跑、会话和 Redis 受管及独立 Worker。发布目录搬移后复验读取原数据与进程退出。
- 104 个映射页面的三语落点和交付计划锚点全部存在，三语构建及 18 项浏览器检查通过。生态 31 条仍按交付计划保留范围待确认。Windows Docker Desktop、生产 TLS、托管 SQS 标准／FIFO 和托管 Pusher 等验收仍引用各自目标环境检查。继续保留 08-04 用户此前确认的模拟正式环境验收结论。
- 独占服务和编译产物已回收，Docker 卷清单一致，磁盘可用 85.67 GB（79.79 GiB）。提交前周额度剩余 21%，按用户授权继续。日志位于 `/private/tmp/openrust-http-release-recheck.yd73YSMP`。

## Windows ARM64 文件锁与 Artisan 源码入口复验

- Parallels Windows 11 ARM64（Build 26200）用 Rust 1.94.0、原生 ARM64 MSVC 14.44.35207。从 `baac80470` 干净归档取实际脚本与框架，叠加本批可移植测试。先按 Cargo.lock 补齐来宾尚未缓存的依赖，再锁定离线构建。
- Windows 上 32 项缓存目标测试通过，1 项子进程夹具默认忽略并由父测试实际调用。未启动 Windows MySQL／Redis 等外部驱动。本地 NTFS 验证跨进程文件锁及独立 Artisan 清理。macOS ARM64 对应 3 项文件缓存与 1 项 Unix 启动器定向测试通过。
- CMD 和 PowerShell 从应用目录之外通过实际 `artisan.bat` 执行成功和失败命令，原生二进制实际 HTTP `GET /up` 返回 200。格式检查、主机严格 Clippy、三语构建和 18 项浏览器检查通过。Windows Docker Desktop 后端仍按既有发布实机要求跟踪。
- 清理本批来宾构建与应用夹具，恢复虚拟机初始停止状态。主机本批缓存已回收，磁盘可用 74.68 GB（69.55 GiB），提交前周额度剩余 20%，按用户授权继续。日志位于 `/Users/Home/openrust-windows-entry.PjLCM8cZ`。

## Docker 非特权运行的包清单路径

- 对照固定 Laravel 的部署目录权限和包清单路径。旧版部署镜像把 `bootstrap/cache` 交给非特权用户。当前 Compose 已持久化 `/app/storage`，镜像通过 `APP_PACKAGES_CACHE` 将包清单放到挂载目录，支持一次性容器共享。保留应用路径和显式环境变量覆盖。 [Laravel 部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md#directory-permissions)
- 在 `3155d59dc` 隔离快照构建 macOS ARM64 独立二进制，用只读发布根目录复现 `package:discover` 原路径权限失败。用新镜像的环境变量实测包清单生成、`optimize`、`config:cache`、`optimize:clear --except=cache`、重复 `clear-compiled`、自定义路径及相邻文件保留。使用配置缓存时实际首页和 `/up` 返回 200，SIGTERM 退出 0，`inspire` 正常。
- Compose 配置解析确认存储挂载。Linux 容器夹具采用生产 Dockerfile 的相同环境变量和 UID 10001，以只读根文件系统及共享存储实际验证原目录拒写、首容器写入和次容器读取及清理。完整 OpenRust Linux 镜像随后通过 [原生 amd64 复验](docker.md#原生-linux-完整镜像包清单复验)和 [ARM64 交叉构建与原生运行复验](docker.md#arm64-完整镜像包清单复验)。Windows Docker Desktop、生产 TLS 和托管 SQS 仍按 [目标环境清单](#剩余实机环境复查) 跟踪。
- Rust 1.94.0 全仓格式、启用 `standalone` 的工作区全目标严格 Clippy、三语站点构建和 18 项浏览器检查通过。临时镜像、编译及站点产物已回收，Docker 卷清单一致。提交前磁盘可用 52.43 GB（48.83 GiB），周额度剩余 19%，第二窗口无读数，按用户授权继续。验收日志保留于 `/Users/Home/openrust-docker-package-cache.ybE1t3Zw`。

## 核心机制复验与成熟方案核对

2026-11-21，从 `b45427830` 干净 Git 归档以 Rust 1.94.0、独占 target、jobs=2、关闭增量和 dev debug 执行定向复验。未纳入其他对话的数据库注释、迁移合并或扩展工作区改动。设计来源、固定提交与采用边界见[架构记录](../../architecture.md#成熟机制复用与适配边界)。

- `cargo +1.94.0 test --locked --offline --test health --test config_cache --test configuration_cache --test container_lifecycle`：10 项通过，包含真实 HTTP／Artisan 的配置缓存、Provider 与 Facade 应用隔离、引导失败及健康路由。
- `cargo +1.94.0 test --locked --offline -p openrust-framework --test routing_groups`：2 项通过，真实 TCP 验证域名、参数约束、方法及有／无／损坏索引；动态路由先注册时仍优先匹配。
- 经 `tests/postgres_acceptance.py` 确认 PostgreSQL 180006，以 REQUIRE 防止缺服务跳过，根 `migrations::artisan_migrations_use_a_real_postgres_ledger` 通过；框架 `database_migrations::postgres_migration_transactions_roll_back_schema_and_data_before_updating_the_ledger` 通过，覆盖失败 up／down 的 Schema、数据和账本回滚。后者显式映射 `OPENRUST_TEST_MIGRATION_POSTGRES_URL` 与 `OPENRUST_REQUIRE_MIGRATION_POSTGRES=1`。

合计 14 项测试函数通过；本批没有变更 Rust 实现，无新增依赖，不重复宣称其他驱动或全仓最终验收通过。公告与许可证盘点见[依赖复核](../dependency-security/README.md#核心整合依赖复核)。源码、日志与上游原文保留于 `/private/tmp/openrust-core-review-20261121/`。联网检索接口未返回正文，改为直接 HTTPS 读取官方仓库固定提交与官方文档；不把搜索摘要当作源码证据。发布最终快照仍须合并后重验。

收尾已确认本批 Cargo 与浏览器进程结束，两个标签化 PostgreSQL 容器已由夹具回收。直接移除独占 target、隔离站点 dist／cache 与本批 node_modules，共4,698,019,149逻辑字节；文件系统实际可用空间从48,298,713,088增至52,840,218,624字节，净增4,541,505,536字节（并行构建可能影响全盘读数）。清理前后 Docker 数据卷清单一致；源码、日志、API交付导出、共享缓存与全部数据卷保留，详见同批 `cleanup.json`。本批仅本地提交。


## 2026-11-21 原生时间、设置与扩展组合复验

最终核心源码固定为 `b168b31aa063a289a5a025ed5080a99f19bb6c7e`，使用独立 Git 归档，包含数据库翻译／设置 S1、统一短信、腾讯社交登录与 MiniApp、PostgreSQL 绑定修复、Jiff/time 原生迁移。同期 Plan 对话新建的支付扩展另行开发，未复制其工作区改动，也未计入本批源码、API 和依赖盘点。设计复用与固定上游依据见[架构记录](../../architecture.md#成熟机制复用与适配边界)、[时间及依赖记录](../dependency-security/README.md)及各扩展审计。

- Rust 1.94.0、增量关闭、独占 target、jobs=2：最终 `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` 和全仓格式检查通过。时间专项的固定 IANA2025b vanguard 数据、198,039 个偏移／DST 对照、三库时间边界、Settings 三库各13用例、真实 Pail HTTP／Artisan 与 MySQL Worker 结果见时间记录；未把忽略的两个邮件夹具或 SQL Server 类型测试计作真实目标覆盖。
- 缓存在 `8131dd07f72f3531bdd7e6d066a0b53270a5ad98` 核心组合上实际运行 file、SQLite database、PostgreSQL18 database、MySQL8.4 database、Redis8，五项退出0；必需 PG／MySQL／Redis 缺失时分别按预期退出101。随后 MiniApp 只增加社交登录／配置和对应文档，不改缓存实现；不将此前缓存命令标成在 b168 上执行。
- 最终 b168 元数据／许可证来源盘点及公告检查通过，无新增 ignore，0 errors／0 warnings；启用图无 chrono。optional lock metadata 仍可含未启用 chrono，详见[依赖盘点](../dependency-security/README.md#最终组合依赖与来源盘点)。
- 三语普通指南与联合 API 构建、每语107导航／109 Markdown、7个CLI拒绝用例及9类失效导出拒绝、三语根／子目录桌面／移动浏览器、11库源码和搜索／方法锚点通过。API的2,013个输入与Git归档完全一致，正式站点和导出保留；见[文档快照](../docs-scope/api-reference.md#2026-11-21-设置原生时间与社交登录最终快照)。

主证据目录 `/private/tmp/openrust-final-integration-20261121/`，包括 `source-final.txt`、`clippy-final.log`、`fmt-final.log`、`cache-results.json`、各驱动日志、最终 API 及普通指南日志。首轮录入提交号有误的 API 产物仅作预检且已删除，最终导出读取 Git 生成的提交号，并逐文件验证归档指纹。

资源收尾已删除本批33个闲置测试可执行文件和时间专属target，实际可用空间分别净增3,701,518,336与6,389,047,296字节（合计约10.09 GB，读数可能受并行构建影响）；明细 `cleanup-test-binaries.json`、`cleanup-cargo-final.json`。保留源码、日志、正式 API／站点交付、共享缓存、IDE target 与全部数据库／数据卷。最终认证复验和资源核对另附于本节。

本批不能替代真实 Windows Docker Desktop／虚拟化、生产 TLS 域名、AWS SQS／IAM、Pusher、云 S3／STS、真实邮件／短信收件人、QQ／微信账号及 SQL Server 服务；历史 Windows／Linux证据也不能自动扩展为此最终组合的全平台通过。没有版本升级、tag、推送、Release或生产部署。

最终认证补验：在同一 `b168b31aa063a289a5a025ed5080a99f19bb6c7e` Git 归档，以 Rust1.94／jobs2／incremental0／debug0 顺序执行 `python3 -B tests/postgres_acceptance.py -- python3 -B tests/init_auth_acceptance.py` 和 `python3 -B tests/mysql_acceptance.py -- python3 -B tests/init_auth_acceptance.py`，两者均退出0，实际服务分别 PostgreSQL18.6、MySQL8.4.13。每库验证默认all集中重建／初始HTTP、重复／force／密钥及迁移保留、单项安装、冲突及不完整配置失败、Sanctum／Passport正负Bearer与Guard隔离，以及Fortify安装／重试／定制／迁移、真实HTTP／CSRF／Session／SMTP／资料／密码／2FA／恢复码／重置／限流／关停；生成应用严格Clippy及feature-off构建通过，两次根feature-off构建也退出0。源码无补改。证据 `/private/tmp/openrust-init-auth-combined-20261121/` 的 `source-revision.txt`、`{postgres,mysql}-result.json`、`feature-off-{postgres,mysql}.log`、`init-auth-{postgres,mysql}.log` 和两库辅助HTTP／步骤日志；生成应用与源码保留。该SMTP为受控夹具，不等同真实收件人投递；本批未执行SQLite完整init-auth，也未将先前三库时间／设置专项代作该项。

最终收尾：认证专用闲置 target 已删除，可用空间36,635,472→46,403,828 KiB，实际净增9,768,356 KiB（约10.00 GB）；明细 `openrust-init-auth-combined-20261121/cleanup-final.json`。与主证据 `volumes-before.txt` 及认证清理记录核对，Docker卷前后均为空列表、删除0；当前开发mysql／postgres／redis容器仍在，专属验收容器均已回收。主对话提交前 `df -kP .` 为46,508,636 KiB可用（47.625 GB／44.354 GiB）。清理读数只记实测净增，不外推为所有并行任务产物总量。


## 2026-11-21 真实验收缺口盘点与模拟环境复验

用户要求找全需要真实目标的项目并先完成模拟验收。本次以核心关闭账本 V06–V18 为入口，复查认证、邮件/通知、缓存/队列、存储、广播、数据库/嵌入服务、平台、文档和发布记录；旧记录中的“未验”按后续证据判断，不重复打开已通过的 SeaweedFS ACL/ObjectOwnership 等范围。新增发现的托管 DynamoDB IAM/角色与商业嵌入端点纳入 V15；支付扩展的商户、客户端与资金边界显式列入本表。

隔离源码 `/private/tmp/openrust-production-simulation-20261121/source` 来自 `244842943baeb03214467bef70a5c9aa1b59aec4`，叠加本次 SMTP/队列/广播测试修正；最终静态检查加入扩展 `9ee4881ca` 的测试变更。扩展实际应用验收使用自己的同基线快照，见[扩展记录](../extensions-simulation/README.md)。Rust 1.94.0、jobs=2、增量关闭、dev/test debug=0；未把并行 Activitylog/Horizon WIP 混入本批结果。所有临时服务使用独立回环端口、容器与测试数据库，不操作开发 mysql/postgres/redis。

### 当前模拟覆盖与实机边界

| 项目 | 本次实际执行／保留证据 | 仍需真实目标的内容 |
| --- | --- | --- |
| V06–08 安装与认证平台 | 本机/Windows历史安装证据保留；最近 PG18.6、MySQL8.4.13完整 init-auth、feature-off、HTTP、Artisan 及生成应用严格检查见上节。本次扩展生成应用在两库继续跑完整 Fortify/Passkeys | Windows 单项 Passport 安装入口、最终同一发行快照的全平台整合；历史 Windows 默认 all 不能替代单项路径 |
| V09 社交身份/OIDC/Passkeys | Socialite 全驱动本地 TCP、MySQL 本地绑定；受控 OP、真实 HTTP PKCE、共享 Session 双副本认领及 Chrome 双 RP SSO/退出；ES256/RS256 虚拟认证器和两库生成应用 | 各家真实应用/账号/审核/权限/设备 code、企业 IdP/租户、物理认证器与平台凭据同步；未交付完整 OP 或 front/back-channel logout，不作为现有功能验收要求 |
| V10 Windows Desktop | 本轮启动既有 Windows11 VM；来宾 exec 随后无法建立 Parallels Tools 会话，未可靠取得当前 Desktop 后端状态。保留历史 ARM64 原生/x64仿真与 Docker 路径证据 | 实际 Windows Docker Desktop Linux 后端、挂载权限、容器生命周期与全应用；本轮不能声称虚拟化/后端实测通过或确认缺失 |
| V11 TLS/部署 | 本地 CA 反代实际 production 模式根应用，TLS1.2/1.3，首页和/up 200，错误 CA/hostname 拒绝；实际 migrate --force/status、inspire、SIGTERM=0 | 最终授权域名/公开可信证书/真实反代拓扑、转发及签名配置、线上实例版本和只读命令核验 |
| V12 SQS | ElasticMQ1.6.15，Standard/FIFO；独立 Artisan+MySQL，overflow off/always/retain 三轮全部通过，含可见性/延迟/重投/顺序/去重/TTL/失败账本/跨后端链和恢复 | AWS托管服务 IAM/STS会话、实际角色/KMS、服务端限制和指标。保留既有独立 SigV4/token 验证，不把模拟器当 IAM 授权验证 |
| V13 广播 | Reverb/Pusher 五项 transport；Chromium+Laravel Echo2.2.6/pusher-js8.6.0 的公共/私有/presence、Guard身份、签名投递、断线重连和新socket重新认证；另以HTTPS反代+WSS重复五项通过 | 托管Pusher app/cluster和部署。浏览器仅固定本批证书SPKI，Node HTTPS按私有CA验证；Reverb签名发布穿过HTTPS。Pusher分支Rust Artisan发布到Reverb的后端一段仍是HTTP，不能声称Rust Pusher出站TLS已验证 |
| V14 邮件/通知 | Mailpit1.27.10真实收件箱、STARTTLS/SMTPS、TLS1.2/1.3握手、535/550、错误CA/hostname和禁止凭据明文降级共12场景；SMTP安全4函数、供应商3函数、7个SMTP/MySQL/Redis/Slack/附件/通知HTTP→Artisan场景 | SMTP最终邮箱和供应商证书/权限；SES、SESv2、Postmark、Resend、Mailgun、Cloudflare逐家发件域名/账号/最终收信及回执；Slack Bot/Webhook真实工作区/权限/收信 |
| V14 短信 | Aliyun/Tencent/Twilio/Vonage/HTTP协议2函数；现有sms_worker在SQLite3.54.1、PG18.6、MySQL8.4.13三库分别覆盖database+Redis队列、签名/命名配置、密钥保护、未知结果不重发、429及SIGKILL前后无重复供应商调用 | 各家真实号码/模板/发送资格、地区网络、最终投递状态和适用退订/合规流程 |
| V15 S3 | RustFS1.0.5三个精确专项：9MiB multipart、Unicode/空对象、并发替换、源错误/取消清理、代理重试、presign上传/下载/临时origin/version | AWS公网、真实凭据链/角色/STS及其他承诺厂商/大对象上限；独立SeaweedFS ACL/ObjectOwnership历史已过，不再列缺口 |
| V15 缓存/队列/DynamoDB | 三库HTTP database queue与Redis reservation/blocking；Redis Cluster TLS错误CA拒绝/hostless redirect；现有完整Cluster脚本6个精确函数，包括新增SQLite账本/HTTP→两个真实Artisan并发消费的队列多键限流；DynamoDB Local3.3.2三函数实际表/TTL/锁/条件并发/自定义属性/Artisan、批量未处理项重试及耗尽 | 托管DynamoDB IAM/角色/TTL后台回收、AWS网络/限额；本地TTL读取过滤不证明托管异步删除时机 |
| V15 数据库/嵌入服务 | 现有SQL Server2017/2022协议/容器历史范围保留；嵌入服务两个函数重新跑实际HTTP，Bearer/model/dimensions/encoding、503恢复、缓存隔离与非法维度/驱动拒绝 | SQLServer最终原生Linux x86_64服务；商业embedding真实端点/账号/模型。当前Linux SSH用户无Docker socket权限，不等于主机没Docker；MySQL8.0客户端→8.4 dump旧失败组合仍保留，匹配8.4已过 |
| 支付/订阅扩展 | 微信/支付宝/Stripe/Paddle实际入站TCP→业务500回滚→重试提交→重复原生ACK；三库并发/取消/乱序和官方固定SDK签名独立重生；Cashier迟到事件查当前供应商状态 | 真实商户/产品/资金、公开回调、App/JSAPI/小程序调起、Checkout/门户、Stripe.js/Paddle.js/SCA及实际支付网络 |
| V16 文档/API | 本次三语SMTP说明、内容检查、构建、普通12+导航6+邮件页12浏览器检查；现有0582cc8bd十三库/2039指纹API快照保留 | 正式发布URL与最终代码指纹一致的API重建；历史导出不包括本次Cargo feature/lock变更或并行新包 |
| V17–18 最终发行 | 本次范围的全workspace/all-target/all-features严格Clippy/fmt、依赖公告和许可证来源盘点；既有worker/调度/打包/原生平台证据按各自revision保留 | 锁定最终干净提交的OS/数据库/生成应用/安装/Worker/调度/打包矩阵，以及最终版本/CHANGELOG/分发许可证/敏感信息审计和用户明确发布批准。人工批准不能模拟 |

### 验收发现与修正

- SMTP使用lettre0.11.24原有WebPKI根，无法信任进程显式提供的私有CA；加启现有`rustls-native-certs` feature，保留内置公共根及链/hostname验证。依赖版本未升级，仅锁项增加lettre的既有依赖边；三语正文说明系统信任与SSL_CERT_FILE/SSL_CERT_DIR。先失败后修正通过，未关闭证书检查。`tests/smtp_acceptance.py`把12场景固定为可重复入口，验证实际收件箱/MIME与失败零投递。
- SQS旧链恢复夹具在三表账本重构后仍预期worker退出1，并额外执行一次耗尽tries。按既有Worker/Laravel CallQueuedHandler语义改为失败后`released_after_exception`/`released`、源队列保留、恢复后发送尾任务；不为旧测试改生产Worker。FIFO每轮使用新队列，避开服务真实五分钟去重窗口。
- Redis Cluster队列限流此前只有单实例验收，新增正式Cluster入口专项；多键与两个并发Artisan只允许一个处理器执行，剩余释放/重试与缺失/无限制策略复用现有断言。
- 最终只读复核后将两个TLS代理的初始化移入既有try/finally，确保第二个初始化失败时第一个也被清理；该收尾调整通过Node语法检查，HTTP/WS和HTTPS/WSS功能结果对应调整前的通过记录，未重新构建。隔离source与输入摘要同步。
- 扩展测试修正自定义Passkeys主键声明匹配，详见独立提交`9ee4881ca`；没有修改扩展产品逻辑。
- 首轮DynamoDB故障代理错误转发空批次及返回空未处理列表，按服务协议修正代理后重跑三项通过；未改变生产重试规则。WSS首轮误将Pusher策略名配置成wss（策略名应为ws，由forceTLS选择wss），修正并加入localhost允许origin后通过。尝试将Rust Pusher发布也指向本地私有CA时返回500；依赖源码显示macOS reqwest0.13默认使用平台验证器，本次SSL_CERT_FILE未使该客户端完成请求；未输出原始TLS错误，根因没有独立确认。没有导入系统信任或关闭校验，最终保留前表所述后端HTTP边界，失败证据继续保留。

### 复现与证据

主目录 `/private/tmp/openrust-production-simulation-20261121/`：`revision.txt`、`run.py`/`run-isolated.log`、三个`sqs-*.log`、`s3-*.log`、`mail-*.log`、`sms-{sqlite,postgres,mysql}.log`与wrapper日志、`smtp-accepted-final/results.json`、`redis-tls.log`、`redis-cluster-final.log`、`dynamodb-success.log`/`dynamodb-verified/`、`embeddings-final.log`、`reverb-tls-accepted.log`／`reverb-plain-accepted.log`、`tls-results.json`、`docs-*.log`。各精确函数只在服务变量实际提供后计通过；provider ignored函数单独调用，不把跳过视为成功。扩展完整命令/双库和官方参考见[扩展记录](../extensions-simulation/README.md)。

复验命令在隔离source，统一传`RUSTUP_TOOLCHAIN=1.94.0`、独占`CARGO_TARGET_DIR`、`CARGO_BUILD_JOBS=2`、`CARGO_INCREMENTAL=0`和两种debug=0。服务测试由保留的`run.py`和`dynamodb.py`建立本批隔离实例；新增入口为`python3 -B tests/smtp_acceptance.py <mail_notifications测试二进制> <不存在的证据目录>`和`sh tests/redis_cluster.sh`。TLS广播以`OPENRUST_REVERB_ECHO_MODULES`/`NODE_PATH`指向固定客户端目录，`DOCS_BROWSER_PATH`指定实际Chromium，`OPENRUST_REVERB_TLS_CERT`/`KEY`指向生成证书，`NODE_EXTRA_CA_CERTS`指向CA，再运行`cargo +1.94.0 test --locked --offline -p openrust-reverb --test transport -- --nocapture`；不设置TLS变量时保留HTTP/WS原路径。证书为短期夹具，过期时重新生成。

本轮重新复查Windows后VM为suspended以保存状态；不能称恢复了最初stopped状态。首个ElasticMQ启动缺少/data映射，Docker新增匿名卷`aa42b7128c5272474510232a6b584a97a7d64bc20ed7ee7ac81ddfb27a8b61c2`已保留，随后使用/data tmpfs，最终服务清理前后卷一致；未删除任何卷。中途一次邮件重编译误从活跃工作树启动，立即中断并在隔离source重启；该中断结果不计验收成功。


最终检查：`clippy-combined.log` 全workspace/all-targets/all-features、`fmt-accepted.log`均退出0，包含9ee4881ca新增回调测试；`advisories-final.jsonl`摘要0 errors/0 warnings、`licenses-final.json`启用642包来源/许可证清单成功（工具提示未找到配置，使用默认策略，未添加ignore）。普通指南`docs-build.log`、`docs-browser.log`12组合、`docs-scope-final.log`6导航组合、`docs-mail-browser-final.log`12个邮件页面均通过。`smtp-accepted-final.log`最终12场景通过；首轮Python3.14严格X509因夹具CA缺keyUsage拒绝，已显式生成CA basicConstraints/keyCertSign/cRLSign和叶证书serverAuth/用途后重跑，未放宽Python验证策略。`reverb-tls-accepted.log`与`reverb-plain-accepted.log`最终各5函数通过。依赖补丁的既有弃用/cfg编译警告仍存在，不能将严格workspace检查通过描述为第三方编译输出没有warning。

上游依据：固定Laravel `CallQueuedHandler`的异常释放行为，以及[lettre v0.11.24 TLS实现](https://github.com/lettre/lettre/blob/v0.11.24/src/transport/smtp/client/tls.rs)、[Mailpit v1.27.10](https://github.com/axllent/mailpit/tree/v1.27.10)、[ElasticMQ v1.6.15](https://github.com/softwaremill/elasticmq/tree/1.6.15)和[AWS DynamoDB Local官方部署说明](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/DynamoDBLocal.DownloadingAndRunning.html)。联网搜索没有返回正文，直接HTTPS取得官方原文存本批目录；不依靠Star或搜索摘要。Mailpit/RustFS/DynamoDB固定镜像digest，ElasticMQ1.6.15也记录实际digest；模拟服务不引入生产依赖或新的框架抽象。


资源收尾：全部本批服务/测试退出且专属target无占用后，删除check-target、普通指南dist与本批echo客户端node_modules；保留source/官方原文/日志/MIME/所有SQLite与数据卷。`cleanup-accepted.json`记录可用空间77,809,025,024→82,819,829,760字节，实测净增5,010,804,736字节（约5.01 GB）；并行活动会影响净值，不当作本批所有产物逻辑总量。开发mysql/postgres/redis仍运行，最终仅保留上述匿名卷，删除0卷。15份本批/扩展代码及三语文档与隔离source逐字节一致，临时`validated-inputs.json`留证；本批不包含并行构建入口/Activitylog/Horizon和默认导航的改动。后续新批按当时新落地的统一build_session规则执行，不追溯声称本批早先已使用该入口。


## 2026-11-21 扩展已实现子集与核心组合复验

收到 Activitylog／Horizon 阶段交付后，以核心 `9eea6eee59134eb6523e069d7e0fddca955d6c02` 叠加当时两包、根 feature／依赖／锁项和三语文档固定源码到 `/private/tmp/openrust-core-ecosystem-20261121/source`，保留此前核心 SMTP 原生证书与邮件／队列测试。临时 `snapshot-inputs.json` 记录输入，`verify.py`、`resume.py` 与逐项日志用于复现，不将这些 WIP 归给基线提交。

用户随后明确要求严格对照 Laravel 生态。扩展对话已重开 O01／O02 差异审计，Horizon 自定义分配算法与固定上游不等价，标签／通知／快照等仍有缺口。本节仅验证固定时点已实现子集的组合运行，不将测试通过表述为 Laravel 功能完整、最终组合验收或新版本 API 导出。修正后须重新固定源码并验收。

本轮在隔离源码运行 Rust 1.94.0，全 workspace／all-targets／all-features 严格 Clippy 与 fmt 通过；沿用 Cargo 原生并发和增量，独占 target 可复用。仅本批验证命令的 dev／test 调试符号设为0，未修改根 IDE 配置或共享缓存策略。

已通过的当前子集：

- 两包8个非忽略配置／持久化／Provider／授权函数；Horizon另2个需Redis函数经专属Redis8.10.3与REQUIRE入口实际执行通过，不把默认忽略计作通过。
- 全feature原生根应用4项：默认骨架2项、Activitylog配置／迁移／缓存1项、Horizon配置／缓存1项；feature关闭的默认骨架2项。
- Activitylog相同持久化2函数与根应用1函数在SQLite、PostgreSQL18.6、MySQL8.4.13分别执行；使用专属数据库包装器，原生子进程实际完成发现／发布／迁移／数据保留／缓存／清理／回滚。SQLite包含在前述包与根配置测试内。
- 独立Redis与Horizon原生监督进程：暂停／恢复、积压扩容、异常Worker补位、失败重试／删除、SIGTERM排空、terminate代次与Redis停止后回收；Chromium1243 headless shell桌面／手机面板授权、刷新、CSP、脱敏与布局通过。

首次显式使用完整Chrome1243时，未授权页面的HTTP403导航抛出`ERR_HTTP_RESPONSE_CODE_FAILURE`；脚本此前的原生HTTP403已通过。去掉executablePath又因运行时默认寻找本机未安装的1234失败；最终显式使用已有headless shell1243通过全部进程与浏览器断言，未下载浏览器或修改生产代码。原失败保留于`process.log`／`process-headless.log`，成功见`process-headless-1243.log`；不能将此记成所有浏览器一致，已向扩展对话同步。

三语内容检查／完整构建、通用12组、完整导航6组（各23个支持边界页）、Activitylog12组、Horizon12组浏览器检查通过。证据保留于`/private/tmp/openrust-core-ecosystem-20261121/`，包括固定source、snapshot-inputs.json、results.json、clippy／extensions／native／redis／process／三库及docs日志。

收尾后开发mysql／postgres／redis仍运行，本轮临时服务已退出；数据库／全部卷保留。独占target约14GiB及文档产物作为修正后组合工作流的可复用缓存登记保留，不重复冷构建或因本轮退出清空。收尾可用约70.96GB（66.09GiB），高于32GB停线，详见临时closeout.json。未修改扩展实现；本次仅核范围提交核心审计与交接记录，不提交并行扩展。最终修正后功能对齐、跨平台与统一API导出仍待新快照，真实账号／设备／商户资金及部署边界沿用前节。

收尾核对2,921份代码／测试／文档输入，其中Horizon的options测试、supervisor／options／repository实现4份已由扩展对话开始修正；其余固定输入一致。该4份新源码未纳入本轮快照通过结论，差异见临时closeout.json。
