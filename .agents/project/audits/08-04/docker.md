# Docker 开发与部署

现行用法见[Docker指南](../../guides/docker.md)及本文末尾[当前核对](#当前配置与关联资料核对2026-11-21)。下列各批结果对应当时提交和配置，不作为旧项目名、profile、端口与环境变量的现行指引。

在 `991ab3ada` 的隔离源码上叠加本批 [Dockerfile](../../../../docker/Dockerfile)、[Compose](../../../../docker/docker-compose.yml) 和当时的公共目录覆盖（现已合并入唯一Compose配置）。沿用 v0.x 的独立发行和非特权运行，参考固定 Goravel 的多阶段镜像及单一应用入口。使用当前 `main` 与 `standalone` 契约。运行镜像随附视图和公共资源，环境在启动时提供，数据使用主机相对目录。开发可只启动 MySQL 和 Redis，集群可使用外部共享服务。迁移由同一镜像的 Artisan 一次性执行。

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

## 原生 Linux 完整镜像包清单复验

- 在原生 Ubuntu 24.04.3 x86_64 用 `12fe39579` 的干净归档和仓库 Dockerfile 构建 `linux/amd64` 完整发布镜像。Rust 1.94.0 锁定 release 构建通过。核对镜像 UID/GID 为 10001，运行镜像无 Cargo 和应用环境文件。沿用旧版非特权容器及固定 Goravel 的单一编译入口。 [Laravel 部署目录权限](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md#directory-permissions)
- 用仓库 Compose 及只读根文件系统覆盖启动完整镜像，按部署文档将挂载的 storage 目录交给 UID/GID 10001。实际 Artisan 强制旧路径时因文件系统只读失败。镜像默认路径可生成 JSON、自定义路径可以单独生成和清理、应用容器重建后默认清单仍可读。后续两次独立 `clear-compiled` 成功并保留相邻存储文件，`inspire` 正常。
- 隔离 MySQL 8.4.13 与 Redis 8 的回环端口通过握手。真实 Artisan 生成密钥、执行 13 项迁移和幂等重跑。两套 Compose 应用共享服务，首页、`/up`、只读 public 文件和迁移状态通过。受管 Redis Worker 与独立 `queue:work redis --once` 各完成实际邮件任务，失败表为空。实际 SIGTERM 退出 0，重建应用保留存储和迁移账本。本地 HTTPS 反向代理使用受信任的测试证书返回 200。
- 复验当前提交的三语部署站点、根路径和 `/docs/` 共 18 项浏览器检查。相同源码的全仓格式和严格 Clippy 见 [包清单权限修复](README.md#docker-非特权运行的包清单路径)。本次实测架构为原生 amd64。Windows Docker Desktop、生产 TLS 和托管 SQS 仍见 [环境与权限](README.md#剩余实机环境复查)。
- 本批独立容器、镜像、构建器、构建缓存及站点产物已回收，远端卷清单一致，隔离数据库文件保存在远端私有目录。远端可用 110.70 GB（103.09 GiB），项目盘可用 52.42 GB（48.82 GiB），提交前周额度剩余 19%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-linux-package-cache.GJmKruxl`。

## ARM64 完整镜像包清单复验

- 用 `c901caf09` 的干净归档和原始 Dockerfile 在 Ubuntu x86_64 交叉构建 `linux/arm64` 完整发布镜像，Rust 1.94.0 锁定 release 构建通过。构建交叉编译应用，BuildKit 模拟 ARM64 运行镜像的系统包安装。导出镜像通过 SSH 传回并校验 SHA-256，在 Apple Silicon 的 Docker Desktop 原生运行。镜像平台为 `linux/arm64`、应用容器内 `uname -m` 为 `aarch64`，UID/GID 为 10001。 [Docker 多平台构建](https://docs.docker.com/build/building/multi-platform/) · [Laravel 部署目录权限](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md#directory-permissions)
- 用只读应用根文件系统及共享 storage 实测 Artisan。旧路径因只读失败，默认路径和自定义路径分别可以生成清单，自定义清理保留默认清单。应用容器重建后默认清单仍在，两次独立 `clear-compiled` 成功并保留相邻存储文件，`inspire` 正常。
- 隔离 MySQL 8.4.13、Redis 8.10.3 及宿主机回环端口通过。真实 Artisan 生成密钥、执行 13 项迁移且重跑无待执行项。两套 Compose 应用共享服务，首页、`/up`、只读 public 文件和迁移状态通过。受管 Redis Worker 和独立 `queue:work redis --once` 各消费一项任务，失败表为空。SIGTERM 退出 0，重建应用保留存储和迁移账本。本地 HTTPS 代理使用受信任的测试证书返回 200。
- 框架源码、Docker 配置和三语站点与 [amd64 复验](#原生-linux-完整镜像包清单复验) 相同，三语构建与 18 项浏览器结果沿用该次证据。发布镜像和本次容器已回收，远端专用构建器及缓存已清理。两端 Docker 卷清单一致，宿主既有容器保留，隔离数据库文件保存在私有目录。项目盘可用 52.33 GB（48.74 GiB），提交前周额度剩余 19%，第二窗口无读数，按用户授权继续。验收日志位于 `/private/tmp/openrust-arm64-package-cache.Hy1tobKR`。

## 单一Compose与根环境文件（2026-11-21）

按用户要求只保留docker/docker-compose.yml，删除dev/public覆盖文件及docker/.env.example。唯一Compose采用现有开发项目名openrust-dev，保留运行中的PostgreSQL18／MySQL8.4／Redis8回环端口与docker/data/dev数据路径；应用使用app profile，默认只启动依赖，可显式指定app或启用profile。静态资源只读挂载根public，可用OPENRUST_PUBLIC_PATH覆盖。插值命令从根目录显式传--env-file .env，应用env_file固定为../.env，不修改用户根.env及已有数据。

参考Docker Compose官方环境插值、服务env_file和profiles机制，复用现有配置而不新增启动包装器。Docker Compose v5.7.1实际config解析通过：根.env供给四服务，默认仅三项依赖；app/profile及public只读路径正确；独立项目tmp夹具验证根.env同时提供插值与应用环境，三项数据库凭据分别缺失均明确拒绝。核对现有三容器的挂载与新配置逐项相同，ps仍为running/healthy；未执行up/down、容器重建、迁移或数据库写入，未重新构建Rust镜像。

三语部署正文、根.env.example、开发服务指引和项目规范同步。项目tmp/compose-unification-20261121/source/docs隔离构建通过（保留现有大chunk提示），通用浏览器12组及部署页专项12组通过，覆盖三语／桌面手机／根及docs子路径；git diff --check通过。未暂存、提交或推送。

## 本地数据库root账号统一（2026-11-21）

按用户当次要求将根.env的DB_USERNAME／DB_PASSWORD改为root，.env.example示例与三语部署说明同步。MySQL改用MYSQL_ROOT_PASSWORD、MYSQL_ROOT_HOST与root健康检查，删除MYSQL_USER／MYSQL_PASSWORD及随机root密码初始化。PostgreSQL继续使用POSTGRES_USER／POSTGRES_PASSWORD读取根配置。

已有两数据库通过实际SQL创建／更新root登录，不删除原应用用户与任何数据；随后仅重建postgres／mysql容器更新环境与健康检查，保留原bind mount。两个数据库均通过host.docker.internal对应宿主映射端口验证root/root登录，错误密码明确拒绝；重建前后数据挂载一致，容器均healthy。Redis未变。未提交推送。

## 可选Docker环境覆盖（2026-11-21）

用户调整环境规则：根.env提供默认值，docker/.env为可选覆盖，后者优先。唯一Compose保持不变；app的env_file采用长格式，依次required根文件与required:false的docker文件（Compose2.24+），不新增环境模板或启动包装器。Compose插值仍使用原生CLI：存在覆盖文件时依次--env-file .env --env-file docker/.env，不存在时仅根文件；服务environment和shell插值优先级保持原生行为。禁止将服务env_file误写成插值来源。

Docker Compose v5.7.1在项目tmp/compose-env-overrides-20261121独立夹具验证六项：覆盖文件缺失、多个字段覆盖、部分字段覆盖、空文件回退、显式空密码拒绝和实际根配置解析，均通过；应用环境、两库凭据和宿主端口按相同文件顺序验证。原用户根.env及数据库实际账号未修改，未重建容器、未访问或删除数据。规范、根环境模板注释、三语部署正文及本机服务指引同步；三语隔离构建、部署页12组与通用12组浏览器验收记录保存在同批tmp目录。未提交推送。

## 当前配置与关联资料核对（2026-11-21）

用户要求保持原生Docker Compose直接启动，不引入Python或启动包装器；曾尝试的版本读取入口及对应专属测试已撤掉。镜像使用`${APP_NAME:-openrust}:${APP_VERSION:-latest}`，不提供单独镜像覆盖变量；Compose不读取Cargo，显式APP_VERSION指定发布标签，否则使用latest。应用自身版本保持既有Cargo默认逻辑。默认五服务（含Nginx），app等待依赖healthy；Redis统一监听／映射／健康检查／应用连接。docker/.env只保存容器连接差异。

核对范围包括Compose、Dockerfile及专属排除文件、根环境模板、README、三语配置和部署、Docker指南、项目规范、上下文、交接和全仓引用。旧审计保留其版本边界；临时夹具前缀和源码历史盘点不作为现行配置。Docker使用Dockerfile.dockerignore，不误报缺少根.dockerignore。

沿用原生Compose多环境文件及service_healthy。在线官方页面检索未返回正文，以本机Compose解析及真实服务验收为准。

本轮实际结果：

- 原生Compose配置矩阵通过：默认latest、显式版本、可选覆盖、Shell优先级、Redis自定义端口、应用固定内部端口、容器命名及健康依赖。不存在持久化Python启动入口或专属包装器测试。
- 依赖容器重建前后，PostgreSQL与MySQL完整导出的规范化结果相同，Redis启动应用前的键空间相同；保留既有数据目录、root/root账号及APP_KEY。应用执行4项待迁移（users、cache、jobs及sms_deliveries），重跑显示Nothing to migrate。
- 原生up启动五服务全部healthy；应用8000与Nginx8080的首页和/up均200，nginx -t通过。真实Redis16379隔离夹具PING与健康检查通过，夹具已回收且数据卷清单未变。
- 三语站点构建通过（仅既有包体积提示）；根路径及/docs/下的三语桌面／手机12项通用检查与12项部署流程检查通过，部署步骤顺序和页面无横向溢出已核对。

证据位于项目tmp/compose-consistency-fix-20261121/，SQL备份保持私有权限；没有删除数据库数据或数据卷，未暂存、提交或推送。本轮实际容器运行平台为本机macOS Docker Desktop；不将其扩展为Windows或远程生产环境的新验收结果。


### 镜像配置变量独立（2026-11-21）

按用户要求，现行镜像表达式改为`${APP_IMAGE_NAME:-openrust}:${APP_IMAGE_VERSION:-latest}`；APP_NAME继续用于应用名称与容器命名，APP_VERSION继续表示应用版本。环境模板、三语部署文档及现行规范同步；上节原变量名为此前验收时配置。此次仅核对配置解析，不重建服务或迁移数据。


### Docker变量前缀统一（2026-11-21）

用户要求Docker相关应用变量使用APP_前缀；存储与公共目录改为APP_STORAGE_PATH、APP_PUBLIC_PATH，Dockerfile构建并发参数改为APP_BUILD_JOBS，镜像与feature参数沿用APP_IMAGE_NAME、APP_IMAGE_VERSION及APP_BUILD_FEATURES。现行配置和三语文档同步，历史验收原变量按历史时点保留。

提交前复核：Docker相关变量统一为APP_前缀，挂载改用APP_STORAGE_PATH／APP_PUBLIC_PATH，构建并发参数改用APP_BUILD_JOBS；现行Compose、Dockerfile及三语部署正文无OPENRUST_变量。默认及自定义镜像／挂载路径配置解析通过，Dockerfile的build --check无警告；三语站点重新构建、12项通用与12项部署页浏览器检查通过。此次参数改名未重新编译完整Rust镜像；沿用此前完整构建运行证据，当前五服务healthy。提交限定Docker相关14文件，共享规范／交接仅暂存Docker段落，其余WIP保留。
