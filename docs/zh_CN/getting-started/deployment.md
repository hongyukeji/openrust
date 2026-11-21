# 部署

## 构建应用

使用 Rust 1.94 或更新版本及仓库锁文件。在目标操作系统与架构构建，或使用另行验收的交叉编译工具链。

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.blade.html release/resources/views/welcome.blade.html
```

将网站资源复制到 `release/public`，公共存储文件复制到 `release/storage/app/public`。欢迎页在运行时读取，必须随二进制部署。Rust 配置和路由声明编译进 `main`，修改 Rust 源码需要重新构建；运行时环境值和视图文件不需要。

`standalone` 特性以可执行文件所在目录作为应用根目录，不依赖工作目录。绝对 `APP_BASE_PATH` 可覆盖该位置。使用进程环境变量，或将 `.env` 放在应用根目录；进程环境值优先。不要公开生产密钥，也不要未经修改就把 `.env.example` 用作生产配置。

## 准备运行环境

启动前配置生产环境值。设置 `APP_ENV=production`、`APP_DEBUG=false` 和生产环境的 `APP_KEY`。为服务账户授予 `storage/` 及本地数据库目录的写权限。将持久化存储和数据库放在可替换的发布目录之外，按需在部署配置中使用绝对路径。

在流量切换到新版本前，用新产物执行迁移：

```sh
./main artisan migrate --force
```

首次部署 SQLite 时先创建配置指定的数据库文件。已有应用继续使用其应用密钥和持久化数据。见[迁移](../database/migrations.md)。

使用 MySQL 或 PostgreSQL 时，先在[发布矩阵](../prologue/releases.md)核对服务端版本。OpenRust 的 MySQL 连接要求 TLS。应用默认启动数据库队列 Worker，使用时需先完成队列表迁移。

## 启动 HTTP 与 Artisan

准备好运行环境并完成必要的迁移后，直接启动应用。HTTP 和 Artisan 都可以在没有预生成缓存的情况下运行。

```sh
cd release
APP_HOST=127.0.0.1 APP_PORT=8088 ./main
```

在另一个终端进入发布目录：

```sh
./main artisan list
./main artisan help inspire
./main artisan inspire
curl -i http://127.0.0.1:8088/up
```

将 `APP_URL` 配置为应用的公开 URL。未设置时，默认本地 URL 的端口取自 `APP_PORT`。公共磁盘使用绝对 URL 时必须与 `APP_URL` 同源。HTTP 监听器在绑定前校验端口；Artisan 无需合法的 HTTP 端口或 HTTP 监听器。

`SIGINT` 以及 Unix 上的 `SIGTERM` 会请求 HTTP 优雅关闭。使用进程管理器负责重启。已验证的平台与驱动见[发布说明](../prologue/releases.md)。

源码的 `artisan` 脚本运行 Cargo。独立发布无需 Cargo，使用 `./main artisan …` 运行命令。参阅 [Artisan](../digging-deeper/artisan.md)和[应用配置](configuration.md)。

### 重新加载服务 {#reloading-services}

部署新版本后，可以请求独立管理的服务优雅重载：

```sh
./main artisan reload
./main artisan reload --except=queue,reverb:restart
```

`reload` 调用 `queue:restart` 和 `schedule:interrupt`。Reverb 服务提供者为独立 Reverb 服务接入 `reverb:restart`。服务提供者也可通过 `reloads` 注册其他任务。用 `--except`（`-e`）传入逗号分隔的任务名称或 Artisan 命令名。

在与 Worker 相同的共享缓存配置下执行。独立 Worker 和 Reverb 退出后由进程管理器重新启动。统一启动模式下，重启整个 `main` 进程以加载新二进制并重新启动内嵌服务。

### 验证发布产物 {#verify-the-release}

应用监听后，在发布目录执行以下检查。HTTP 请求使用部署配置的监听端口。

```sh
curl --fail http://127.0.0.1:8088/up
./main artisan migrate:status
./main artisan schedule:list --json
```

迁移状态会列出待执行的迁移。调度列表以 JSON 数组返回已注册的任务，未注册任务时返回空数组。

## 可选的部署优化

需要预生成部署缓存时，可以使用以下命令。

### 缓存配置 {#cache-configuration}

部署环境配置就绪后，可以在启动服务前生成配置缓存：

```sh
./main artisan config:cache
```

在最终发布目录中，使用服务启动时的环境执行命令。配置快照可能包含绝对路径。搬移发布目录或修改配置后，先在新环境中重新生成缓存，再重启服务。需要在启动时从环境读取配置时，执行 `./main artisan config:clear`。见[配置缓存](configuration.md#configuration-caching)。

### 缓存部署资源 {#cache-deployment-resources}

```sh
./main artisan optimize
./main artisan optimize:clear --except=cache
```

`optimize` 缓存配置、事件发现结果、路由匹配索引和编译视图。`optimize:clear` 清除这些产物、包清单和默认应用缓存。用 `--except`（`-e`）传入逗号分隔的任务名或 Artisan 命令名。示例会保留默认应用缓存。服务提供者可通过 `optimizes` 添加任务。

覆盖路由、事件或包清单缓存路径时，在服务和运行 Artisan 的 Shell 环境中设置对应的 `APP_ROUTES_CACHE`、`APP_EVENTS_CACHE` 或 `APP_PACKAGES_CACHE`。配置缓存命中后，启动可能跳过读取 `.env`。生成、读取和清理这些文件时应使用同一组路径。视图编译目录取自配置快照。

参见[路由缓存](../the-basics/routing.md#route-caching)、[事件发现](../digging-deeper/events.md#event-discovery-in-production)和[包发现](../digging-deeper/packages.md)。

## 一起运行服务

直接运行 `./main` 启动 HTTP、默认异步队列的 Worker，以及已注册任务的调度器。`sync` 连接会跳过 Worker。gRPC 等监听器遵循各自的配置。启动前先准备队列后端。使用数据库队列时，先执行上面的迁移命令。

设置 `APP_DISABLED_RUNNERS=queue,schedule` 可停用队列和调度。逗号分隔的列表写入 `app.disabled_runners`，空列表允许所有符合条件的 Runner。内置签名为 `http`、`queue`、`schedule`、`grpc` 和 `reverb`。可使用 `q*` 等通配符模式匹配签名。模式支持 `*`、`?`、`[a-z]` 和 `[^a-z]` 等字符类，以及反斜杠转义。`*` 和 `?` 不跨越 `/`。应用会警告并跳过无效模式。所有 Runner 都被过滤或不符合条件时，应用正常退出。

Provider 通过 `ServiceProvider::runners` 提供 Runner，应用可通过 `withRunners(|| vec![...])` 加入自定义 Runner，多次调用时以最后一次传入的回调为准。相同签名以首次注册为准，Provider 先于应用自定义列表。应用先过滤禁用签名，再调用 `shouldRun`。符合条件的 Runner 在应用进程内并发运行。

Ctrl+C、Unix SIGTERM 或 Runner 运行失败时，应用按 `shutdownPriority` 从小到大关停，同组并发，默认优先级为零。`run` 可通过 `RunnerContext` 取消令牌完成在途工作，自定义 `shutdown` 可释放其他资源。`run` 成功返回后仍会执行关停钩子，以支持自行启动后台工作的 Runner。

框架新增的常驻服务在已装配、已配置且满足启动条件时，默认纳入主应用启动。各服务仍可关闭或独立运行。一次性 Artisan 命令由显式调用执行。

```sh
./main artisan queue:work
./main artisan schedule:work
./main artisan schedule:run
```
HTTP、受管 Worker 和调度器共用应用进程。受管 Worker 沿用独立 `queue:work` 的超时、重试和退出码。进程 RSS 包含三个服务的内存占用，因此受管 Worker 不套用独立内存门槛。独立 `queue:work` 保留现有内存上限。启用强制超时的任务仍由隔离执行器运行。Ctrl+C 或 Unix SIGTERM 会停止接收新工作，并等待在途 HTTP 请求、队列任务和调度任务完成。应用进程会传递 Worker 的非零退出码。

## Windows 独立部署 {#windows-standalone}

使用目标架构的 Rust 和 MSVC Build Tools 构建发布产物。全新 SQLite 部署先准备可执行文件、资源、公共文件和环境，再创建数据库并执行迁移。

```powershell
cargo +1.94.0 build --locked --release --features standalone
New-Item -ItemType Directory -Path release, release/database, release/storage/app/public
Copy-Item target/release/main.exe release/
Copy-Item resources, public release/ -Recurse
Copy-Item .env.example release/.env
New-Item -ItemType File release/database/database.sqlite
$env:APP_ENV = 'production'
$env:APP_DEBUG = 'false'
$app = Join-Path $PWD 'release/main.exe'
& $app artisan key:generate --force
& $app artisan migrate --force
& $app artisan inspire
& $app
```

后续发布应保留应用密钥、数据库和可写存储。服务管理器可从其他工作目录运行 `main.exe`，也可用 `APP_BASE_PATH` 指定绝对应用根目录。

## TLS 终止 {#tls-termination}

将应用绑定在私有地址，由反向代理或负载均衡器终止 TLS。把 `APP_URL` 设为公开 HTTPS 地址，并配置代理转发公开主机名和协议。从客户端验证证书链和主机名，检查 HTTPS 首页和健康路由。


## Docker 部署 {#docker-deployment}

Linux 使用带 Compose 和 Buildx 的 Docker Engine，macOS 和 Windows 使用 Linux 容器模式的 Docker Desktop。同一份 Dockerfile 支持构建 Linux `amd64` 和 `arm64` 镜像。以下命令从仓库根目录执行，Windows PowerShell 第一步用 `Copy-Item docker/.env.example docker/.env`。

Windows ARM64 的 Docker Desktop 后端需要可运行的 WSL 2。Windows 虚拟机需要虚拟化平台提供嵌套虚拟化。参见 [Docker Desktop 系统要求](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements)。

本地开发可只启动 MySQL 8.4 和 Redis，应用在宿主机运行。根目录的应用 `.env` 设置 `DB_CONNECTION=mysql`、`DB_HOST=127.0.0.1`、`DB_PORT=33060`、`REDIS_HOST=127.0.0.1` 和 `REDIS_PORT=16379`；MySQL 库名、账号和密码与 `docker/.env` 一致。使用 Redis 缓存和队列时设置 `CACHE_STORE=redis` 和 `QUEUE_CONNECTION=redis`。应用密钥为空时先生成密钥，再执行 `./artisan migrate` 和 `cargo run`。Windows PowerShell 使用 `.\artisan.bat`。

单机部署应用容器的步骤如下。

1. 复制环境模板。生产环境在初始化 MySQL 前设置 `APP_ENV=production`、`APP_DEBUG=false`、强密码 `DB_PASSWORD` 和公开 `APP_URL`。妥善保存 `docker/.env`。
2. 启动依赖并构建应用镜像。用容器的 Artisan 生成应用密钥，填入 `docker/.env` 的 `APP_KEY`。升级和多实例部署继续使用原密钥。

```sh
cp docker/.env.example docker/.env
docker compose --env-file docker/.env -f docker/docker-compose.yml --profile local up -d --wait mysql redis
docker compose --env-file docker/.env -f docker/docker-compose.yml build app
docker compose --env-file docker/.env -f docker/docker-compose.yml run --rm app artisan key:generate --show
```

3. 应用进程使用 UID/GID 10001，`docker/data/storage` 需要对该用户可写。Linux 上新建的目录可执行 `sudo chown 10001:10001 docker/data/storage`。Docker Desktop 上确认目录已共享给 Docker 且在容器内可写。执行一次迁移，再启动应用。

```sh
docker compose --env-file docker/.env -f docker/docker-compose.yml run --rm app artisan migrate --force
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --wait app
```

访问 `http://localhost:8000/up`，端口可通过 `APP_PUBLISHED_PORT` 修改。宿主机端口默认绑定回环地址，便于在前面接反向代理。MySQL、Redis 和应用存储默认挂载到 `docker/data/` 下的相对目录。需要只读挂载静态资源时，在 Compose 命令中加上 `-f docker/docker-compose.public.yml`，默认挂载仓库的 `public/`，由宿主目录提供完整的公共文件。

镜像默认设置 `APP_PACKAGES_CACHE=storage/framework/cache/packages.json`，包清单保存在挂载的 `storage` 中。一次性容器运行 `package:discover` 或 `clear-compiled` 时也使用该路径。自定义路径时，选择挂载目录中的可写位置。

多机部署使用同一应用镜像，省略 `local` profile。设置 `OPENRUST_IMAGE`、共用的 `DB_HOST` 和 `REDIS_HOST`，各实例使用一致的应用密钥、缓存和队列配置及凭据。拉取或载入镜像后用 `up -d --no-build --wait app` 启动。每次发布集中执行一次迁移，入口由负载均衡器处理 TLS，上传文件使用 S3 等共享存储。如果调度器只需运行在一台实例上，其他实例设置 `APP_DISABLED_RUNNERS=schedule`。构建时用 `APP_BUILD_FEATURES` 选择 `reverb`、`sanctum` 等可选功能。
