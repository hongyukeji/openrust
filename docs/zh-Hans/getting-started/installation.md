# 安装

## 环境要求

安装 Rust 1.95.0 或更高版本及 Cargo。OpenRust 使用 Rust 2024 edition，根应用和 `vendor/openrust/framework/` 中的框架 crate 一起构建。首次构建会下载 `Cargo.lock` 中记录的依赖。

## 完整初始化 {#complete-initialization}

先准备 MySQL 数据库和 Redis 服务，再通过进程环境或应用环境文件提供连接。MySQL 使用 `DB_URL`，或提供 `DB_DATABASE` 和 `DB_USERNAME`。Redis 使用 `REDIS_URL` 或 `REDIS_HOST`。按实际服务补充主机、端口、密码和 TLS 设置。

```sh
./artisan init:full --dry-run
./artisan init:full
cargo run --locked
```

`init:full` 全程非交互，写入文件前检查配置。它启用完整预设及网页和 API 用户套件，安装包资源，构建应用，执行迁移及 HTTP、Artisan 和 Redis Worker 自检。缓存、会话和队列使用 Redis。MySQL、Redis 和 Docker Compose 由命令外的环境管理。通过进程环境注入的凭据仍由进程环境提供，启动应用时也需要提供。已有驱动设置发生冲突时须先显式调整。`init:full --force` 用于明确授权生产迁移，并保留已有源码定制。需要交互选配时使用 `./artisan init`。

`init:full` 通过包发现启用[机器翻译包](../packages/translate.md)。应用需要机器翻译时，再配置具名服务并提供对应凭据。

单机和多副本使用同一套应用配置。同一部署内各运行角色共用 MySQL、Redis 和应用密钥。运行角色、迁移协调和文件共享由部署工具管理。

## 启动应用

在应用根目录执行以下命令。创建新的本地环境时复制环境模板。

```sh
cp .env.example .env
touch database/database.sqlite
cargo +1.95.0 build --locked
./artisan key:generate
./artisan migrate
./artisan inspire
cargo +1.95.0 run --locked
```

环境模板使用 SQLite 存储应用数据、会话、缓存和队列。使用这些服务前先创建数据库文件并执行迁移。

HTTP 默认监听 `127.0.0.1:8000`。访问 `http://localhost:8000/` 查看欢迎页，访问 `/up` 检查服务状态。使用 Ctrl+C 停止服务。

应用也能在没有环境文件时启动。需要其他端口时，在 `.env` 中设置 `APP_PORT=9081`。使用模板的默认监听主机时，`APP_URL` 会展开为 `http://127.0.0.1:9081`。进程环境优先于文件中的同名值，例如 `APP_PORT=9081 cargo +1.95.0 run --locked` 会监听 9081。URL 覆盖和环境文件选择见[配置](configuration.md)。

Windows 可用 `copy .env.example .env` 复制模板，再执行 `artisan.bat inspire`。使用目标架构的 MSVC Rust 工具链。独立发布可按 Windows 部署步骤运行。 [Windows 独立部署](deployment.md#windows-standalone)

## 构建独立发布产物

```sh
cargo +1.95.0 build --release --locked --features standalone
```

将 `target/release/main` 放在发布根目录，随包提供 `resources/views/welcome.blade.html`。网站文件按原路径放在 `public/`，公共磁盘文件放在 `storage/app/public/`。可执行文件从所在目录读取 `.env`。应用根目录在其他位置时，通过进程环境设置绝对路径 `APP_BASE_PATH`。在产物所在目录运行 `./main artisan inspire`，也可以使用可执行文件的绝对路径。源码和独立产物的目录解析规则见[请求生命周期](../architecture-concepts/lifecycle.md)。

依赖下载后可添加 `--offline` 进行锁定离线构建，构建仍需完整的应用和框架源码。
