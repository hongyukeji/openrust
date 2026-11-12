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

默认运行 `./main` 启动 HTTP 及其已配置的监听器。后台服务可独立运行，使用 `./main artisan queue:work`、`./main artisan schedule:work` 或 `./main artisan schedule:run`。

要用一条命令启动 HTTP、Worker 和调度器，在应用环境中设置以下值，然后运行 `./main`。

```dotenv
RUNTIME_ENABLED=true
RUNTIME_HTTP=true
RUNTIME_QUEUE=true
RUNTIME_SCHEDULE=true
```

统一启动开启后，各服务开关默认都是 `true`，设为 `false` 可停用对应服务。Worker 使用默认队列连接及其默认队列，跳过使用 `sync` 驱动的连接。应用注册了调度任务才启动调度器。gRPC 等监听器仍遵循各自的配置。没有可运行的服务时，启动返回错误。

Ctrl+C 或 Unix 的 SIGTERM 会停止接收新的 HTTP 请求、队列任务和调度任务，并等待在途工作完成。任一受管服务退出也会触发其余服务关闭。Worker 配置为任务超时后退出时，应用进程保留它的超时退出码。统一启动的 Worker 与其他服务共享进程，因此关闭 Worker 的进程内存检查。需要独立内存上限、重启策略或命令参数时，使用独立的 Worker。 在共享运行时中执行阻塞代码会拖慢 HTTP 和调度。此类任务可由独立 Worker 消费，也可使用队列的强制超时隔离执行。
