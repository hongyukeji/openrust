# 安装

## 环境要求

安装 Rust 1.94.0 或更高版本及 Cargo。OpenRust 使用 Rust 2024 edition，根应用和 `vendor/openrust/framework/` 中的框架 crate 一起构建。首次构建会下载 `Cargo.lock` 中记录的依赖。

## 启动应用

在应用根目录执行以下命令。创建新的本地环境时复制环境模板。

```sh
cp .env.example .env
touch database/database.sqlite
cargo +1.94.0 build --locked
./artisan key:generate
./artisan migrate
./artisan inspire
cargo +1.94.0 run --locked
```

环境模板使用 SQLite 存储应用数据、会话、缓存和队列。使用这些服务前先创建数据库文件并执行迁移。

HTTP 默认监听 `127.0.0.1:8000`。访问 `http://localhost:8000/` 查看欢迎页，访问 `/up` 检查服务状态。使用 Ctrl+C 停止服务。

应用也能在没有环境文件时启动。需要其他端口时，在 `.env` 中设置 `APP_PORT=9081`。模板中的 `APP_URL="http://localhost:${APP_PORT}"` 会展开端口变量。进程环境优先于文件中的同名值，例如 `APP_PORT=9081 cargo +1.94.0 run --locked` 会监听 9081。URL 覆盖和环境文件选择见[配置](configuration.md)。

Windows 可用 `copy .env.example .env` 复制模板，再执行 `artisan.bat inspire`。Windows 实机运行仍待验收。

## 构建独立发布产物

```sh
cargo +1.94.0 build --release --locked --features standalone
```

将 `target/release/main` 放在发布根目录，随包提供 `resources/views/welcome.html`。网站文件按原路径放在 `public/`，公共磁盘文件放在 `storage/app/public/`。可执行文件从所在目录读取 `.env`。应用根目录在其他位置时，通过进程环境设置绝对路径 `APP_BASE_PATH`。在产物所在目录运行 `./main artisan inspire`，也可以使用可执行文件的绝对路径。源码和独立产物的目录解析规则见[请求生命周期](../architecture-concepts/lifecycle.md)。

依赖下载后可添加 `--offline` 进行锁定离线构建，构建仍需完整的应用和框架源码。
