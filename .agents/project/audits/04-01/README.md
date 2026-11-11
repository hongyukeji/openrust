# 04-01 路由参数、分组及诊断

## 契约与实现

- 对照固定 Laravel Router、RouteGroup、RouteCollection、RouteListCommand 与测试。参考 Goravel route:list，并从旧版 `6d1a8192` 的路由集合用例提取命名、方法顺序、同路径不同域名和覆盖语义的验收场景。 [Laravel Routing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md) · [Goravel route:list](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/route/console/route_list_command.go)
- 应用门面接通分组与方法列表。分组继承 URI、名称、域名、参数约束和中间件，支持域名变量及尾部可选路径参数。原有路由注册回调和 `Route::get` 签名保留。Rust 分组回调显式借用 Router，约束方法使用 `r#where`。
- Artisan `route:list` 支持文本、JSON、筛选、排序与反向输出。输出包含生效方法、域名、URI、名称、处理器、注册位置和已声明中间件。Rust 注册位置由编译器提供，处理器采用 Rust 类型名或 Closure，供应商筛选使用注册源路径。
- 命名 URL 生成、路由中间件展开详情、任意 PCRE 语法兼容与 Windows 实机尚未验收。复杂动态域名与控制器动作元数据随对应交付继续核对。

## 验收

隔离基线 `21a65e98` 加已验收依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，测试报告 399 项通过。此批未启用真实 MySQL、Redis 和 S3。
- 真实回环 HTTP 验证 GET、POST、HEAD、OPTIONS、405 的 Allow、错误域名和无效约束 404、可选参数默认值、同一路径不同域名的方法隔离，以及分组结束后的外部路由。
- 三语路由示例均编译并通过真实 HTTP、Artisan JSON 筛选；独立应用执行 `./artisan route:list --json --method=HEAD`、`./artisan route:list --except-path=up,missing --sort=definition` 和 `./artisan inspire` 成功。
- 三语文档站通过构建、12 项通用浏览器检查、6 项路由正文专项检查，覆盖根路径与 `/docs/`。

下一批 04-02 依赖本批路由入口。
