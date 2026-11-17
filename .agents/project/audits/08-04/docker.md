# Docker 开发与部署

在 `991ab3ada` 的隔离源码上叠加本批 [Dockerfile](../../../../docker/Dockerfile)、[Compose](../../../../docker/docker-compose.yml) 和[公共目录覆盖](../../../../docker/docker-compose.public.yml)。沿用 v0.x 的独立发行和非特权运行，参考固定 Goravel 的多阶段镜像及单一应用入口。使用当前 `main` 与 `standalone` 契约。运行镜像随附视图和公共资源，环境在启动时提供，数据使用主机相对目录。开发可只启动 MySQL 和 Redis，集群可使用外部共享服务。迁移由同一镜像的 Artisan 一次性执行。

| 范围 | 实测结果 |
| --- | --- |
| 平台 | macOS ARM64 上的 Docker Desktop 29.9.3、Compose 5.7.0、Buildx 0.38.1。实际构建并运行 `linux/arm64` 与 `linux/amd64` 的发行镜像。AMD64 交叉构建，在 Docker Desktop 兼容环境中运行。 |
| Compose 与生产迁移 | 两个架构均在独立 MySQL 8.4.13 和 Redis 8 中通过。实测 Artisan 生成密钥、`migrate --force` 执行 12 项迁移、重复执行为空、首页和 `/up` 返回 200。 |
| 应用运行 | 核对运行 UID 10001，运行镜像无 Cargo 和环境文件。只读 public 映射文件可访问，storage 相对映射可写。主进程管理的 Redis Worker 消费排队邮件，并在日志中核对对应主题和零失败记录。 |
| 集群和重建 | 每个架构启动两个独立 Compose 应用，共用 MySQL 和 Redis。第二个应用正常服务并读取同一迁移账本。SIGTERM 退出为 0，重建应用保留存储和迁移记录。独立重建 MySQL／Redis 后，从宿主机测试 MySQL 8.4 握手和 Redis PING，迁移账本 12 行保留。 |
| TLS 与文档 | AMD64 镜像经本地 HTTPS 反向代理和验证过的自签证书访问 `/up` 成功。三语文档构建与根路径／子路径 12 项通用浏览器验收通过。 |

Windows PowerShell 启动步骤已记录，Windows 路径使用 Docker Desktop 的 Linux 容器模式。正式环境证书继续按发布矩阵验收。

BuildKit 直连镜像分发和 crates.io 时遇到网络失败，隔离 builder 和构建的代理由本次验收环境提供，项目交付文件未写入代理地址。两轮失败的 builder 和最终 builder 均已清理，临时应用镜像、容器、Compose 网络和隔离文档构建产物均已移除。保留验收日志和挂载数据库数据。清理前后宿主可用空间实测由 106.96 GB 增至 113.15 GB，净增 6.19 GB。

## Windows 本机模拟验收

在 Parallels Windows 11 ARM64 的原生 PowerShell 5.1.26100.9457 运行 Docker Compose v5.7.1 官方 ARM64 可执行文件，核对发布摘要后执行。实际从包含空格的 Windows 目录复制环境模板并解析 Compose，默认和带空格的自定义相对挂载路径、只读 public 覆盖、镜像和宿主端口覆盖、local profile 和外部服务模式均通过。验证使用 Windows Compose 客户端的配置命令，Linux 容器运行证据来自上面的 macOS Docker Desktop 和 Linux 验收。Windows Docker Desktop 后端及其文件挂载仍待可运行 WSL2 的 Windows 宿主验证。已移除测试 exe 和 Windows 夹具，并恢复虚拟机原先的停止状态。

## 原生 Ubuntu x86_64 实机

在 Ubuntu 24.04.3 LTS x86_64 主机上，以 `57bab2532` 的隔离源码和同一批 Docker 配置运行验收。Docker Engine 29.9.3、Compose 5.7.1、Buildx 0.39.0。Rust 1.94.0 原生 release 构建和 `linux/amd64` 运行镜像通过。

- 在隔离 MySQL 8.4.13 和 Redis 上验证宿主机开发端口、Artisan 密钥生成、生产 12 项迁移和重复迁移。实际访问首页和 `/up`，核对非 root 运行、只读 public、storage 相对挂载读写，以及 Redis Worker 处理排队邮件。
- 两个独立应用实例共享 MySQL 和 Redis。SIGTERM 正常退出，重建应用保留存储，使用同一宿主目录重建数据库和 Redis 后，迁移账本仍有 12 条记录。用本地测试证书验证 HTTPS 反向代理请求；正式环境证书待发布验收。
- 容器、镜像、BuildKit 缓存和移交源码包已清理。验收通过后，BuildKit 资源自动移除，sudo 启动的 Docker 客户端留下的 root 属主登记目录按精确路径补清。复核 Docker 临时资源与 BuildKit 缓存均为 0。本地保留验收日志；远端的 MySQL／Redis 绑定数据保存到私有持久目录。宿主可用由 105.41 GB 增至 111.98 GB，实测净增约 6.57 GB。

## 原生 Ubuntu x86_64 发布复验

复用历史验收的 Ubuntu 24.04.3 LTS x86_64 主机，现有 SSH 登录和免交互 Docker 权限可用。使用 `07307c6e159b8f8b97e999d4439fe9ea6dd091e3` 的 Git 归档，Docker Engine 29.9.3、Compose 5.7.1 和 Buildx 0.39.0 在原生 x86_64 上完成 Rust 1.94.0 锁定 release 构建。使用该提交的 Dockerfile 和 Compose 文件启动独立部署。初轮隔离夹具的解包掩码导致镜像内视图权限不足。复验按 Git 归档的原始文件模式解包。

- MySQL 8.4.13 和 Redis 8.10.3 的宿主回环端口可用。真实 Artisan `key:generate --show`、`migrate --force`、幂等重跑、`migrate:status` 和 `inspire` 通过，迁移账本共 12 条。首页、`/up` 和挂载的公共文件均返回 200，UID 为 10001，公共目录只读，存储目录可写。
- 默认 Runner 和独立 Artisan `queue:work redis --once` 分别处理隔离的 Redis 邮件任务。独立 Worker 返回当前任务的成功事件，两条队列均消费完成且失败账本为空。
- 两个 Compose 应用实例共用 MySQL 与 Redis，两个实例的首页及健康路由均通过。SIGTERM 退出码为 0。重建应用后存储仍可读，重建 MySQL／Redis 后迁移账本仍有 12 条记录。通过本地测试证书访问 HTTPS 代理的 `/up`。
- 本批 Compose 容器、专用 Buildx 构建器和缓存、应用镜像、临时源码与站点已清理。复查 Docker 卷清单无变化，MySQL 和 Redis 绑定数据保留在远端私有目录。远端可用空间清理后为 103.31 GiB，比构建器清理前增加 7.05 GiB。三语站点构建、12 项通用导航和 6 项发布矩阵浏览器检查通过。
