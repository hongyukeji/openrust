# 部署

## 构建应用

使用 Rust 1.94 或更新版本及仓库锁文件。在目标操作系统与架构构建，或使用另行验收的交叉编译工具链。

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.html release/resources/views/welcome.html
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

## 启动 HTTP 与 Artisan

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
