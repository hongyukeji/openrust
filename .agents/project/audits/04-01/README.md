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


## route:list 中间件诊断

- 对照固定 Laravel `RouteListCommand` 的详细级别和 `gatherRouteMiddleware`。旧版 `v0.x` 有命名中间件注册、分组及验收，未找到 `route:list` 的对等输出。本批复用现有应用配置、别名、分组与 HTTP 路由执行的解析器。 [固定 Laravel 命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/RouteListCommand.php)
- `-v` 输出 Rust 应用在路由上登记的中间件名称和组名。`-vv` 展开组，应用排除、优先级排序与去重，显示实际 Rust 中间件类型。参数化别名保留参数，可终止中间件显示其底层类型。诊断单独准备解析配置，HTTP 路由的运行状态不受影响。默认 JSON 保持已有字符串数组，`--middleware` 按所选视图筛选。
- 隔离基线 `91db2996`，Rust 1.94.0。15 项 HTTP 回归通过，其中真实回环请求与命令复用同一应用装配，验证嵌套组、执行顺序与重复中间件。Artisan 集成覆盖无详细参数、`-v`、`-vv --json`、有效类型筛选、排除和参数保留。
- 实际应用二进制验证普通、`-v`、`-vv` 与两个 JSON 形式，运行三语文档的 `route:list -vv --json --middleware=SubstituteBindings` 和 `artisan inspire`。三语站点与 18 项浏览器检查通过。证据目录 `/private/tmp/openrust-04-01-route-list.SHPuOhOH`。
- 命名 URL 在 [06-01](../06-01/README.md) 另有验收。04-01 的动态域名与控制器动作诊断继续跟踪，任意 PCRE 表达式兼容和 Windows 实机尚未覆盖。
- 严格工作区 Clippy 与改动文件格式通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412`。本批 3.7 GiB 隔离 Cargo 产物已回收，磁盘可用 112.22 GB（104.51 GiB），周额度剩余 65%，第二窗口无读数。


## 路由绑定字段与动作诊断

- 对照固定 Laravel 的 `RouteUri::parse`、`RouteCollection::addToCollections` 和 `RouteListCommand::resolveUri`。旧版 `v0.x` 有路由集合和注册用例，未找到对等的 Artisan 输出。Rust 保留已登记的 URI 供 URL 生成和命令诊断，另为含绑定字段的模板生成注册键。注册键保留参数名及可选标记，绑定字段独立交给解析器。 [Laravel RouteUri](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/RouteUri.php) · [Laravel 路由集合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/RouteCollection.php)
- 方法、域名和注册键相同时覆盖对应的生效方法。文本与 JSON 均显示动态域名、绑定字段和 `setAction` 登记的动作。Rust 同时保留可选绑定字段的 `?`；默认输出仍以现有生效方法和 JSON 字段结构为准。
- 隔离基线 `58ae2ab0`，Rust 1.94.0。16 项应用 HTTP 回归与 4 项框架 URL 回归通过。真实请求验证同一动态域名上的 GET 使用新 `slug` 绑定、POST 保留旧 `id` 绑定、可选参数及错误域名。Artisan 用相同注册组合核对方法、URI、域名、动作及筛选。
- 实际二进制运行三语共用的 `route:list --domain=example.test --action=PostController --json` 示例，另验证普通文本、应用 `-vv` 示例和 `artisan inspire`。三语站点及 18 项浏览器检查通过。证据目录 `/private/tmp/openrust-04-01-route-metadata.gpU7sdLZ`。
- 严格工作区 Clippy 与改动文件格式通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412`。本批 4.2 GiB 隔离 Cargo 产物已回收，磁盘可用 112.20 GB（104.49 GiB），周额度剩余 64%，第二窗口无读数。
