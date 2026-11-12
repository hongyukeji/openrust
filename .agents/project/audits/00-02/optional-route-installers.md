# 00-02 补交 API 与广播路由安装器

- 复核固定 Laravel 13.x 的 [API 安装器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ApiInstallCommand.php) 和 [广播安装器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/BroadcastingInstallCommand.php)。旧版 `6d1a8192` 未发现同名安装器。
- 复用 08-03 已验收的编译型安装流程。OpenRust 修改应用源码和静态装配，随后重新构建。API 认证、频道规则和广播驱动由应用配置。Laravel 的 PHP 依赖安装与交互流程未映射到本批安装器。
- 从 `fa3d54c6` 叠加核定文件。Rust 1.94.0 锁定离线工作区 538 项、格式及严格 Clippy 通过。独立 Artisan 进程验证并发安装、失败时保留自定义接线、重复安装保留用户编辑和无重复声明。
- 使用生成模块重新构建并启动 HTTP。默认 `/api/probe` 和自定义 `/v1/internal/probe` 返回 204，未挂载路径返回 404。使用独立内存会话和测试密钥，`POST /broadcasting/auth` 对无规则请求返回 403。实际 `artisan inspire` 成功。
- 执行三语正文的安装命令。VitePress 构建、12 项通用浏览器检查和 6 项 Artisan 页面定向检查通过。

## 应用骨架默认加载 API 路由

- 根应用内置 `routes/api.rs`，复用现有安装器模板的注册签名和默认 `/api` 前缀及 `api` 中间件组。默认文件由应用声明端点。旧源码仍可通过 `install:api` 补充模块和启动接线。固定 Laravel 13.x 通过 `install:api` 启用，OpenRust 按应用需求将路由文件纳入默认骨架。 [Laravel API Routes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#api-routes)
- macOS ARM64、Rust 1.99.1 的 `cargo test --locked --offline --test startup --test http` 共 37 项通过。验证在默认骨架重复安装保留三个文件，旧骨架仍可安装。实际二进制验证首页和 `/up` 返回 200，缺失 API 路径返回 JSON 404，缺失 Web 路径返回 HTML 404，独立 `artisan inspire` 及 Ctrl+C 关停均退出 0。
- Rust 1.94.0 工作区格式检查、三语文档构建及根路径和 `/docs/` 的 12 项浏览器检查通过。本批执行定向测试。

## API 路由前缀配置

- `config/routing.rs` 将 `API_PREFIX` 载入 `routing.api_prefix`，默认 `/api`。显式 `Routing::apiPrefix` 优先，首尾斜杠移除，空前缀保留 `api` 中间件组。装配解析出的同一前缀用于路由挂载、默认 CORS 和 `withExceptions`。CORS 和异常匹配沿用前缀下的路径语义，空前缀时只关闭自动 API 路径匹配。显式 `cors.paths`（含主机映射和空列表）、默认 `sanctum/csrf-cookie` 及 `expectsJson()` 各自保留其行为。 [Laravel API Routes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#api-routes)
- macOS ARM64、Rust 1.99.1。实际 TCP 验证默认 `/api`、环境变量多级前缀、显式 Builder 覆盖不同环境值、显式空值、环境空值和仅含斜杠、以及自定义 CORS 主机路径。测试路由在生效路径返回 204，旧路径返回 HTML 404。默认预检只匹配生效前缀下的路径，生效前缀下的缺失路由返回 JSON 404，普通 Web 404 保留 HTML，`Accept: application/json` 可请求 JSON。根应用真实二进制用进程环境复核默认值、多级值和空值的 HTTP、CORS、404、独立 Artisan 和 SIGINT。
- 配置、启动、HTTP 及中间件生命周期共 42 项定向测试通过。原 `install:api` 实现和模板保持原样，真实命令验证默认骨架中保留已存在文件、原成功输出及退出 0，旧骨架安装与自定义启动拒绝由回归覆盖。Rust 1.94.0 工作区格式检查、三语站点构建和 12 项浏览器检查通过。本批验收范围为定向回归。

## API 路由使用 Facade 注册

- API 与 Web 共用无参 `register() -> Result<(), RoutingError>` 和应用作用域内的 `Route` Facade。API 回调在有效前缀和 `api` 中间件组中执行。固定 Laravel 在对应分组内载入路由文件，固定 Goravel 由无参回调声明路由。旧版应用显式传入 Router。本批通过 `Route::getWith` 在作用域内配置命名、授权、绑定和路由级中间件。 [Laravel ApplicationBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/ApplicationBuilder.php#L186-L220) · [Goravel WithRouting](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application_builder.go#L164-L168)
- 隔离副本以 `837a9a4c` 为基线。真实 TCP 验证默认前缀、`ROUTING_API_PREFIX` 多级前缀和不同环境值下的 Builder 显式覆盖。API 中间件以响应头验证，Web 路由保留自己的分组。空前缀、默认及按主机 CORS 预检、JSON／HTML 404 和显式 JSON 请求通过。该基线的 CORS 默认配置为所有路径，本批保留配置和实现。
- 实跑 `install:api`。预置骨架保留原文件、输出和退出码，缺接线骨架恢复同样源码并通过构建。仅在隔离安装后的路由文件加入命名探针，实际二进制验证 `GET /v1/installation-probe` 返回 204，`route:list` 显示名称及 `api` 分组，旧路径返回 HTML 404，独立 `artisan inspire` 和 SIGINT 正常。
- macOS ARM64、Rust 1.99.1 锁定离线定向测试 56 项通过，含授权、模型绑定、CSRF、分页、会话认证、API 资源与中间件生命周期。Rust 1.94.0 格式检查、三语构建及 12 项浏览器检查通过。
