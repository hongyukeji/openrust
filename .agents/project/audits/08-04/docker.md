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

## 成熟方案调研与代理边界完善（2026-11-21）

以官方源码、文档与实际行为为判断依据，不按Star数直接引入方案。OpenSpring目录只作辅助对照；本轮未修改该项目。

| 参考 | 核对版本／提交 | 采用与取舍 |
| --- | --- | --- |
| [Laravel Sail](https://github.com/laravel/sail/tree/2688e6018a6421e5aa68ec4008bdcddb607342c1) | 1.x，2688e6018a6421e5aa68ec4008bdcddb607342c1 | 核对compose.stub与MySQL／Redis模板；保留分服务、环境变量及健康检查。Sail服务面向PHP开发环境，不搬入其启动包装器、PHP/Xdebug及源码挂载运行方式。其MYSQL_USER=root设置不适用本项目root账号约定。 |
| [Laradock](https://github.com/laradock/laradock/tree/0fd82a558b314346be12bd9ad9325abae218478c) | master，0fd82a558b314346be12bd9ad9325abae218478c | 借鉴独立服务和按需选择；不复制大量可选服务及其配置层。保持用户要求的唯一Compose，部分启动按服务名选择。 |
| [serversideup/docker-php](https://github.com/serversideup/docker-php/tree/ec58b57bd546004446cb8d0bc7dd070c35f23b41)／[Laravel指南](https://serversideup.net/open-source/docker-php/docs/laravel/laravel-automations) | main，ec58b57bd546004446cb8d0bc7dd070c35f23b41 | 核对非特权运行、健康检查与显式迁移策略。Laravel自动迁移默认关闭；本项目保留单次Artisan迁移，不增加PHP／S6运行环境。未复制其代码，不新增包依赖。 |
| [Docker awesome-compose Rust示例](https://github.com/docker/awesome-compose/tree/503c0dcef31b07fb77b2d4c5556bfafdf86f4f1a/react-rust-postgres) | master，503c0dcef31b07fb77b2d4c5556bfafdf86f4f1a | 多阶段编译、精简运行镜像与当前方向一致。样例仍包含buster与PostgreSQL12，不直接复用版本及生产配置；保留本项目MSRV和PG18。 |
| [Docker Rust指南](https://docs.docker.com/guides/rust/develop/)／[启动顺序](https://docs.docker.com/compose/how-tos/startup-order/)／[日志驱动](https://docs.docker.com/engine/logging/configure/) | 2026-11-21读取 | 沿用BuildKit缓存、service_healthy和原生Compose。实查当前Docker日志驱动为local，已有轮转，不再重复增加日志配置。 |
| [Nginx核心模块](https://nginx.org/en/docs/http/ngx_http_core_module.html#client_max_body_size)／[代理模块](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_read_timeout) | 2026-11-21读取 | 请求体及空闲超时在现有配置中直接声明，不引入模板渲染或新环境变量层。 |

发现及修复：

- APP_BUILD_JOBS原来只定义于Dockerfile，Compose未传参；增加对应build.args映射，默认4，实测设置2后解析值为2。
- 1.5 MiB POST /up直连应用返回405，经原Nginx却返回413，确认代理默认上限提前拦截。显式client_max_body_size 8m，对齐framework http/request/input.rs的8 MiB输入限制。初轮以300秒代理空闲超时完成流式验收；最终配置保留Nginx默认60秒，文档说明使用心跳或按需调整，不新增固定长超时。
- 三语部署文档补齐构建参数、挂载权限、代理边界、源码／镜像更新、保留APP_KEY、单次迁移和回滚的数据兼容限制。

验收使用项目tmp/docker-review-20261121/独占夹具，不启动或迁移开发数据库：官方Nginx与已有PHP镜像仅提供隔离HTTP协议后端，不成为应用部署依赖。真实1.5 MiB请求及Host／scheme／client转发、超过8 MiB拒绝、WebSocket 101握手、首个SSE即时送达全部通过，初轮300秒配置下65秒无数据后第二事件通过（不作为最终默认60秒配置的验收结论），nginx -t通过，专属容器及网络已回收。Compose config与自定义build参数解析通过；未重编当前共享WIP Rust镜像，Dockerfile中其他协作者的许可证改动保留。本轮观察到既有app与nginx已正常退出，未擅自恢复其运行状态；代理验收结果不冒充当前开发服务已启动。

资料下载及测试日志位于项目tmp/docker-review-20261121/；三语隔离构建通过（仅既有包体积提示）；根路径与/docs/下12项通用及12项部署页浏览器检查通过，git diff --check通过。未新增启动脚本、Python运行依赖或Composer／Cargo依赖，未提交推送。

## 开发与部署统一根环境（2026-11-21）

本条覆盖此前可选docker/.env覆盖规则。按用户最终要求：根.env为唯一运行配置；Docker专用参考模板放docker/.env.example，文档说明按需合并至根.env，不能复制覆盖已有根文件或创建第二份docker/.env运行配置。普通应用模板仍在根.env.example，简短连接注释为“容器连接”。

采用Compose原生environment与嵌套插值，不新增入口脚本、运行依赖或环境加载机制。DB_HOST／DB_PORT／REDIS_HOST保留宿主连接；DOCKER_DB_HOST／DOCKER_DB_PORT／DOCKER_REDIS_HOST只覆盖容器目标，未设置或为空时复用普通字段，再回退postgres／5432／redis。MySQL填写mysql／3306；外部服务省略容器覆盖即可复用实际地址。容器监听与宿主绑定保持既有分工。

已把旧docker/.env的四项有效设置合并到根文件：DB_HOST与REDIS_HOST映射为DOCKER_DB_HOST与DOCKER_REDIS_HOST；APP_NAME及APP_URL保留此前Docker实际值，保持现有容器身份和公开地址。旧文件已移除，宿主DB_HOST和REDIS_HOST仍为127.0.0.1；对照既有应用容器环境，APP_KEY、数据库账号／密码／库名、容器名称、连接目标及URL均一致，不输出秘密或额外持久化配置副本。

10项原生配置验收通过：宿主与容器PG、MySQL及不同宿主端口、外部地址复用、空覆盖回退、缺省服务目标、Shell覆盖、自定义Redis端口一致、旧Docker环境文件不生效、根文件缺失拒绝，以及当前迁移配置保留身份和密钥。现有镜像用唯一根文件执行真实Artisan migrate:status成功，四项迁移均Ran；没有执行新迁移、建删表、重建数据库或删除数据卷。三语文档及现行规范、上下文、指南和交接同步；最终文档构建通过，根路径与/docs/下三语桌面／移动端的12项通用浏览器检查和12项Docker部署流程检查全部通过，仅有既有构建分块体积提示。

证据在项目tmp/docker-root-env-20261121/；临时测试中的Python不作为Docker启动依赖，正常启动仍为docker compose --env-file .env -f docker/docker-compose.yml up -d --wait。当前未提交推送。
