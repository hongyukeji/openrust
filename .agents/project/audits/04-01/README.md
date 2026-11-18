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

## 路由约束表达式

- 对照固定 Laravel 的路由约束及 Symfony 的命名参数捕获。旧版 `v0.x` 未发现对等的正则参数约束引擎，本批沿用当前路由注册、域名编译和参数提取。常规表达式使用原有 `regex` 线性路径。环视和命名反向引用由 `fancy-regex` 处理，回溯上限 100,000 步，超出时作为未匹配处理。Rust 保留参数约束末尾的转义字面量。 [Laravel 参数约束](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#parameters-regular-expression-constraints) · [Symfony RouteCompiler](https://github.com/symfony/routing/blob/v7.4.0/RouteCompiler.php)
- Rust 1.94.0 使用 `8d7a31a6d` 隔离快照通过 16 项真实 HTTP 及 4 项 URL 回归。验证环视、命名反向引用、转义美元符、参数捕获、约束不满足时的 404，以及无效表达式保留原路由。实际二进制执行 `route:list -vv --json --middleware=SubstituteBindings` 和 `artisan inspire`。PHP 8.5 与 Symfony v7.4.0 对照验证两个扩展表达式的匹配、捕获及拒绝结果。
- PHP 对照接受 `(*MARK:word)`，当前 Rust 引擎拒绝注册。任意 PCRE 等价和 Windows 实机仍待后续验收。本批保留 04-01 开放及账本 55/66。
- 严格工作区 Clippy 和改动文件格式通过。全仓格式仍只报告既有的 `sanctum_authentication.rs:1412`。三语站点构建和 18 项浏览器检查覆盖共同约束正文及根路径、子路径。证据目录 `/private/tmp/openrust-04-01-route-constraints.0vH6r7Lq`。
- 新增 `fancy-regex` 0.19.2 采用 MIT，最低 Rust 1.66。传递依赖 `bit-set`、`bit-vec` 0.8.0 采用 Apache-2.0 或 MIT。`cargo deny --offline check` 的来源与禁用项检查通过，基线与本批均报告默认许可清单拒绝及既有 `paste`、`pin-utils` 维护提示。本批新增的三条许可提示对应上述三个依赖。

## 显式约束的 PCRE2 匹配

- 固定 Laravel 用 UTF-8 Symfony 路由编译器。显式约束改走 PCRE2，沿用 Symfony 的非捕获组转换和 Rust 的转义字面量保留规则。普通路由继续走原有线性引擎。PCRE2 启用 UTF-8 和 Unicode 属性，解释执行的匹配、深度和堆限制分别为 100,000、1,000 和 8 MiB。限额错误按未匹配处理。 [Laravel 路由编译](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Route.php#L1389) · [PCRE2 资源限制](https://www.pcre.org/current/doc/html/pcre2pattern.html)
- 对照 PHP 8.5 的 PCRE2 10.44 和 Symfony v7.4.0 编译结果，实测 `(*MARK:...)`、占有量词、Unicode 属性和 Unicode 十进制数字，复验环视及命名反向引用。真实 HTTP 同时检查匹配时的参数与约束不满足时的 404。
- `pcre2` 0.2.11、`pcre2-sys` 0.2.10 的包装代码采用 Unlicense 或 MIT。绑定可使用系统 PCRE2，也可编译随包提供的 PCRE2 10.46。本批使用捆绑库验证独立构建，仅使用普通解释器匹配。上游公开的 JIT、DFA、模式转换及序列化接口未参与路由匹配。 [PCRE2 上游版本说明](https://github.com/PCRE2Project/pcre2/releases)
- PHP 对照确认 `/pair/{id}/{slug}` 的 `slug` 可用 `\k<id>` 引用前一个路由变量。Rust 目前提前单独校验每项约束，跨变量反向引用仍待接通。不同 PCRE2 版本和发布平台仍需复验。04-01 保持开放，账本维持 55/66。
- Rust 1.94.0 在 `22fc4cf0e` 隔离快照通过 16 项 HTTP、4 项 URL 回归、实际 `route:list -vv --json --middleware=SubstituteBindings` 及 `artisan inspire`。工作区严格 Clippy、改动文件格式和 18 项浏览器检查通过。全仓格式仍报告既有的 `sanctum_authentication.rs:1412` 差异。`cargo deny --offline check` 的来源与禁用项通过，许可清单与既有依赖维护告警仍报告失败。隔离产物已回收，证据目录 `/private/tmp/openrust-04-01-pcre.9SFIcm2K`。

## 跨参数反向引用与整单元验收

- 固定 Laravel 将路径与域名分别交给 Symfony 编译。OpenRust 保留约束的预校验，只把 PCRE2 的未解析子模式引用延后到完整路径或域名编译阶段。直接路由约束、分组和全局模式都使用同一流程。引用仍未解析时，路由变更或注册失败，原路由及方法索引保持可用。旧版 `v0.x` 未发现对应表达式引擎，路由集合验收场景沿用本单元既有用例。 [Laravel 路由编译](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Route.php#L1389) · [Symfony 路由编译器](https://github.com/symfony/routing/blob/v7.4.0/RouteCompiler.php)
- Symfony v7.4.0 与 PHP 8.5 对照通过命名引用、相对引用、尾部可选参数和域名捕获。实际 HTTP 对照通过同一组成功与 404，并覆盖全局模式和分组前缀。回归确认无效语法即时拒绝，无法解析的引用在完整路由编译时失败。
- 依交付计划核对 GET、HEAD、OPTIONS、404、405、必填与可选参数、分组作用域、文本与 JSON `route:list`，以及绑定字段、动作、域名和中间件诊断。结合前批具名 URL、模式编译及本批跨变量引用实测，04-01 核心单元闭合，账本更新为 56/66。不同 PCRE2 版本及 Windows 实机留到发布矩阵复验。
- `faa57a979` 隔离快照以 Rust 1.94.0 和捆绑 PCRE2 运行，16 项 HTTP、4 项 URL、实际 `route:list -vv --json --middleware=SubstituteBindings` 和 `artisan inspire`、严格 Clippy、三语站点及 18 项浏览器检查通过。改动文件格式通过，全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。已清理本批独占产物，证据目录 `/private/tmp/openrust-04-01-cross-ref.nSix2LRX`。

## 持久化路由匹配索引

- 对照固定 Laravel v13.40.0 的 `RouteCacheCommand` 和 `RouteClearCommand`。旧版 `openrust-v0.x` 未实现持久化路由匹配缓存。OpenRust 的 `route:cache` 将当前编译注册得到的路由索引原子写入 JSON，按 HTTP 方法和路径首段划分候选，动态首段单独保存。匹配时按方法内的原注册顺序合并候选，并继续使用现有域名、约束及绑定逻辑。处理器及中间件由编译后的 Rust 代码在启动时注册。 [固定 Laravel RouteCacheCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/RouteCacheCommand.php) · [固定 Laravel RouteClearCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/RouteClearCommand.php)
- 缓存读取时验证版本、方法集合、索引顺序、位置及分桶覆盖。文件缺失、不可读、损坏或与当前布局不符时回到常规匹配。真实 HTTP 对照无缓存、有效缓存和索引位置越界的缓存，覆盖域名、数字约束、可选参数、HEAD、OPTIONS、405、404 和动态／字面路由重叠。真实 MySQL 8.4.13 用启用缓存的 HTTP 应用通过 Eloquent 绑定回归。
- 在隔离快照 `b8681c8db` 用真实应用二进制与 Redis 8.10.3 验证 `APP_ROUTES_CACHE` 覆盖、`route:cache`、`route:list`、幂等 `route:clear`、`optimize` 和 `optimize:clear`。首页及 `/up` 通过。20 项应用 Console／Artisan、3 项路由回归及 1 项真实 MySQL 回归通过。Rust 1.94.0 全仓格式、工作区全目标严格 Clippy、三语站点及 18 项浏览器检查通过。临时服务和构建产物已回收。磁盘可用 85.12 GB（79.27 GiB），提交前周额度剩余 26%，第二窗口无读数。日志保留于 `/private/tmp/openrust-route-cache.fz4DW2yH`。
