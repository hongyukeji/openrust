# Sanctum-03 API 安装

默认应用骨架移除 API 路由文件与注册调用。`install:api` 通过现有 Rust 源码安装器生成 `routes/api.rs` 并补齐模块和启动注册。保留 `config/api.rs`、`API_PREFIX` 和 API 中间件组。固定 Laravel 的安装命令用 Composer 安装包并发布迁移。OpenRust 通过 Cargo 功能开关编译注册 Sanctum 及迁移，安装命令生成源文件后提示重新构建。旧版 CLI 的路由模板也采用源码生成与编译装配，当前保留无参 Facade 注册方式。Passport 与 Composer 参数仍由后续集成单独跟踪。 [固定 ApiInstallCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ApiInstallCommand.php) · [Laravel API Routes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#api-routes)

隔离副本用实际 Artisan 验证默认无 API 路由、首次生成、重复安装、保留已有文件、`--force` 覆盖、`--without-migration-prompt` 跳过及交互终端拒绝和默认同意。SQLite 核对拒绝时未创建表，默认同意时执行迁移。重建生成后的应用，真实 HTTP 在默认 `/api/user` 和 `API_PREFIX=api/admin` 的 `/api/admin/user` 均验证有效 Bearer 返回 200、缺失或错误凭据返回 401、另一前缀返回 404。用户模型的密码和记住令牌字段未出现在响应。

Rust 1.94.0 的格式、启用 `sanctum` 的全工作区测试（621 次通过、0 失败，含测试启动的夹具）、全目标严格 Clippy、默认功能的安装器回归及三语站点构建均通过。18 项浏览器检查覆盖三语导航和安装文档入口。提交前额度剩余 94%，磁盘可用 59 GiB。
