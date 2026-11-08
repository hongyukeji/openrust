# 安装与运行

先准备 Rust stable 工具链。最低 Rust 版本以根 `Cargo.toml` 的 `rust-version` 为准；仓库的 `rust-toolchain.toml` 声明开发工具链。

在仓库根目录检查 `.env.example` 并配置自己的 `.env`，不要提交部署密钥。标准应用默认使用 Redis 缓存，需准备可达的 Redis；数据库及其他外部服务只在选择相关能力时配置，缺少必需依赖会使启动失败。

```bash
cargo run
```

默认应用监听 `127.0.0.1:3000`，可用部署变量覆盖：

```bash
SERVER_PORT=4000 cargo run
```

应用启动后可请求 `/health`、`/ready` 和 `/api`。默认 `/api` 是根应用信息端点；它不依赖业务模块是否启用。探针可达不代表数据库、队列等全部业务流程已经验证。

不要把运行中的进程当作源码热加载器。编辑 Rust 源码或代码配置后需重新编译，调整部署变量后需重启。

## 执行应用命令

源码仓库和生成项目均提供 `./artisan`。在 POSIX shell 中运行以下命令查看实际登记的命令和用法。

```bash
./artisan list
./artisan help migrate
```

需要设置应用密钥时执行 `./artisan key:generate`。使用数据库前先配置连接，再执行 `./artisan migrate`。仅在私有终端使用 `./artisan key:generate --show`，避免密钥进入日志。

直接运行 `./artisan` 默认执行 `list`，只传 `--env=local` 时也会列出命令。发布时将 `artisan` 与应用二进制 `app` 安装在同一目录，使用 `./app` 启动常驻进程。[源码与发布用法](source-application.md#查找应用命令)

对应固定 Laravel 13.x 官方章节：[installation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/installation.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。
