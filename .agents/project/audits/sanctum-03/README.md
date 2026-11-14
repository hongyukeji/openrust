# Sanctum-03 API 安装

默认应用骨架移除 API 路由文件与注册调用。`install:api` 通过现有 Rust 源码安装器生成 `routes/api.rs` 并补齐模块和启动注册。保留 `config/api.rs`、`API_PREFIX` 和 API 中间件组。固定 Laravel 的安装命令用 Composer 安装包并发布迁移。OpenRust 通过 Cargo 功能开关编译注册 Sanctum 及迁移，安装命令生成源文件后提示重新构建。旧版 CLI 的路由模板也采用源码生成与编译装配，当前保留无参 Facade 注册方式。Passport 与 Composer 参数仍由后续集成单独跟踪。 [固定 ApiInstallCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ApiInstallCommand.php) · [Laravel API Routes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#api-routes)

隔离副本用实际 Artisan 验证默认无 API 路由、首次生成、重复安装、保留已有文件、`--force` 覆盖、`--without-migration-prompt` 跳过及交互终端拒绝和默认同意。SQLite 核对拒绝时未创建表，默认同意时执行迁移。重建生成后的应用，真实 HTTP 在默认 `/api/user` 和 `API_PREFIX=api/admin` 的 `/api/admin/user` 均验证有效 Bearer 返回 200、缺失或错误凭据返回 401、另一前缀返回 404。用户模型的密码和记住令牌字段未出现在响应。

Rust 1.94.0 的格式、启用 `sanctum` 的全工作区测试（621 次通过、0 失败，含测试启动的夹具）、全目标严格 Clippy、默认功能的安装器回归及三语站点构建均通过。18 项浏览器检查覆盖三语导航和安装文档入口。提交前额度剩余 94%，磁盘可用 59 GiB。

## 首次安装补充验收

`install:api` 将既有的 `sanctum` feature 加入应用 `Cargo.toml` 的 `features.default`，保留原有功能和注释。源码编辑共用独立文件锁，保证原子替换 Cargo 清单之后仍可串行处理安装。首次通过交互提示确认迁移时，安装器先重新构建，再通过同一 Artisan 执行 `migrate`。原有 `config/api.rs` 和 `API_PREFIX` 继续控制挂载位置。

在 macOS 隔离副本中使用未启用 Sanctum 的骨架运行 `./artisan install:api`。在交互提示发送回车前，MySQL 8.4.13 中没有 `personal_access_tokens` 表。确认后自动重建，创建令牌表并写入迁移账本，命令退出码为 0。启动生成后的应用，设置 `API_PREFIX=api/admin`，确认匿名访问 `/api/admin/user` 返回 401，`/api/user` 返回 404。定向启动测试验证不可自动接线时保留原 Cargo 清单，以及重复安装、覆盖和保留已有默认 feature。

### Windows 首次安装

在 Parallels Windows 11 ARM64（构建 26200.9457）中，用 Rust 1.94.0 和 ARM64 MSVC 原生构建 `1d712dfe`。每项都从无 API 路由、未默认启用 Sanctum 的骨架开始，并使用独立的空 MySQL 8.4.13 数据库。在 Win32 控制台中通过原生回车事件确认迁移。

| Windows 入口 | 回车后的实际结果 | 生成路由的 HTTP 验证 |
| --- | --- | --- |
| CMD `artisan.bat install:api` | 自动重建、迁移成功，退出码 0 | 默认前缀 `/api/user`，匿名和无效 Bearer 均为 401，有效 Bearer 为 200 |
| PowerShell `.\artisan.bat install:api` | 自动重建、迁移成功，退出码 0 | 自定义前缀 `/api/admin/user`，匿名和无效 Bearer 均为 401，有效 Bearer 为 200 |

两项均确认回车前数据库为空，回车后在独立的临时 target 目录重建并执行迁移。令牌表写入第一批迁移账本，临时重建目录在命令退出时清除。生成的路由使用 `auth:sanctum`，有效令牌返回正确用户且隐藏密码，并更新 `last_used_at`。
