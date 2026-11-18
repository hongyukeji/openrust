# 04-02 请求、响应和控制器

## 契约与实现

- 对照固定 Laravel Request、InteractsWithInput、ControllerDispatcher 和响应文档，参考 Goravel 的请求输入与文件契约。复用旧版 `6d1a8192` 的查询变量树、UTF-8 校验、multipart 分离文本与文件、请求体限额及原始正文复用规则。统一业务响应沿用此前迁入的 ApiResponse。 [Laravel Requests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md) · [Laravel Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md) · [Laravel Controllers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/controllers.md) · [Goravel Request](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/request.go)
- 顶层正文输入优先于查询，保留显式 null。普通 GET/HEAD 使用查询，JSON 媒体类型优先选 JSON。支持精确 query、点号 input、JSON 专用读取、表单数组与 multipart 文件。正文最多缓冲 8 MiB，成功后复用字节。无效编码和正文映射为 400，超量映射为 413。Rust 使用类型化错误拒绝畸形 JSON 和非容器标量。
- Rust 通过显式控制器绑定与类型化方法指针完成应用容器分派。响应支持 JSON、状态码、类型化响应头与异步流。上传元数据与文件字节分开读取。大文件磁盘暂存、文件保存辅助方法、更多类型化输入、资源控制器约定和 Windows 实机仍待覆盖。

## 验收

隔离基线 `21a65e98` 加已验收依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，测试报告 402 项通过。本批未启用真实数据库或外部存储。
- 真实回环 HTTP 覆盖 JSON 与查询冲突、空值、表单、GET 查询、multipart 二进制上传、两个应用控制器实例隔离、重复读取原始正文、400、413、自定义响应头和分块流。
- 三语控制器示例分别编译并发出 JSON、表单及无效正文请求。实际运行 `./artisan route:list --json --method=HEAD` 与 `./artisan inspire` 成功。
- 三语文档站通过构建、12 项通用浏览器检查及 6 项请求、响应、控制器正文专项检查。

下一批为 04-03 中间件顺序、别名和 CORS，依赖 04-01 路由入口。

## 整数、浮点和布尔输入

- 对照固定 Laravel 的 `InteractsWithData`。复用 `v0.x` 的 `6d1a81924` 中 PHP 标量转换，补上严格校验。Rust 显式传入键、默认值和严格模式，沿用现有异步输入和错误传播。三类读取都使用相同的点号路径和正文优先级。共享数字前缀扫描器供 Request 和 Eloquent 使用。 [固定 Laravel 类型化读取](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php#L260)
- PHP 8.5 调用固定 Laravel trait，对照 13 组宽松及严格结果。真实 HTTP 覆盖相同的缺失值、显式 null、容器、数字前缀、前导零、极小指数、整数边界及布尔值。另验证表单覆盖同名查询键、GET 查询和无效 JSON 的 400。MySQL 8.4.13 复验同一扫描器的 Eloquent 数值与向量转换。
- 04-02 继续跟踪 Stringable、日期、枚举、数组与剩余控制器契约，核心账本维持 56/66。
- `8c9e5948d` 的隔离快照以 Rust 1.94.0 运行 `request_input` 真实回环测试、MySQL `eloquent_models`、实际 `route:list --json --method=HEAD` 与 `artisan inspire`。严格 Clippy、改动文件格式、三语站点和 18 项浏览器检查通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。MySQL 夹具与本批产物已回收，日志留在 `/private/tmp/openrust-04-02-scalar.ojacQXuJ`。

## Stringable、集合与枚举

- 对照固定 Laravel 的 `string`、`str`、`array`、`collect`、`enum` 和 `enums`。从旧版 `6d1a81924` 的 `support/typed.rs`、`backed_enum.rs` 和 `scalars.rs` 提取字符串转换、弱类型 backing 与按键过滤。沿用当前异步输入与保键 Collection。单键或整组输入使用 `Option<&str>`，指定多键的选择另行跟踪。 [固定 Laravel 输入方法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php)
- PHP 8.5 使用固定 Laravel trait 和 Stringable，对照默认值、标量转数组、键和重复项、未知 case 与非法类型。真实 HTTP 另验证正文覆盖同名查询键。Rust 用 `r#enum` 保留方法名称，非法 backing 作为客户端输入错误映射到 HTTP 400。
- 04-02 后续为日期、时间间隔、指定多键与剩余控制器契约。核心账本维持 56/66。
- `41f1738dc` 隔离快照用 Rust 1.94.0 通过 3 项真实 HTTP、实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、改动文件格式、三语站点和 18 项浏览器检查。全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。本批产物已回收，日志保留于 `/private/tmp/openrust-04-02-input-values.ZNyHgVFP`。

## 日期与多键选择

- 对照固定 Laravel 的 `date`、`only`、`except` 以及 `array`、`collect` 的多键分支。日期复用框架内的 timelib 求值与时区数据，从旧版 `6d1a81924` 提取 PHP 格式扫描和 Carbon 数字时间戳转换。Rust 从参数或应用配置取时区，保留输入时区和微秒。空值返回 `None`，无效日期映射到 400。 [固定 Laravel 输入方法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php)
- 多键接口复用点号路径读取与 `Arr` 写入、删除规则。Rust 的 `arrayOnly`、`collectOnly` 表达 PHP 方法接收键数组的分支，multipart 文件继续经上传对象读取。真实 HTTP 验证嵌套命中、保留显式 null、省略缺失键、查询补入与移除子键。
- PHP 8.5 `DateTimeImmutable` 对照通过固定偏移、IANA 时区、相对日期、微秒和负时间戳，另验证格式与自由表达式的失败。如果继续核对 Carbon 特有行为，沿用旧版转换用例。04-02 尚待时间间隔与控制器契约，核心账本维持 56/66。
- `eb1193f99` 隔离快照使用 Rust 1.94.0 通过 4 项真实 HTTP、3 项 Calendar 回归、实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、改动文件格式、三语站点和 18 项浏览器检查。全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。本批产物已回收，日志保留于 `/private/tmp/openrust-04-02-date.IKxksx7z`。

## 请求时间间隔

- 对照固定 Laravel 的 `interval` 和 Carbon 3.8.4。从旧版 `6d1a81924` 提取 ISO、自然语言与显式单位解析及其黄金用例，复用当前请求输入、标量转换和 Calendar 预算。区间保留各日历分量、带符号的微秒和方向。显式单位下的 90 秒仍保存在秒分量内。 [固定 Laravel interval](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php#L369) · [CarbonInterval 3.8.4](https://github.com/briannesbitt/Carbon/blob/3.8.4/src/Carbon/CarbonInterval.php)
- PHP 8.5 使用固定 trait 与 Carbon 3.8.4 对照 10 组结果，旧版 240 条黄金用例移入框架测试并通过。真实 HTTP 验证 ISO 和自然语言区间、单位数值前缀、带符号微秒、空值、正文优先级和非法单位。无法解析的区间返回 400，超出解析预算返回 413。剩余复杂 timelib 相对指令保留在兼容边界，未覆盖的表达式报错。
- 下一批核对资源控制器与剩余响应契约，04-02 保持开放，核心账本维持 56/66。
- `086b95415` 隔离快照以 Rust 1.94.0 通过 5 项真实 HTTP、14 项 Calendar 测试、实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、改动文件格式、三语站点和 18 项浏览器检查。全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。本批 Cargo 与文档产物已回收，日志留在 `/private/tmp/openrust-04-02-interval.w2QYPZyV`。

## 常规与 API 资源控制器

- 对照固定 Laravel 的 `ResourceRegistrar` 和控制器文档。旧版 `v0.x` 未找到可移植的资源注册器，本批复用当前 `ControllerDispatcher`、路由分组、参数绑定字段和支持层单数转换。Rust 通过 `ResourceController` 明确提供类型化方法指针，注册前一次校验所有动作和路由配置。 [固定 Laravel 注册器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResourceRegistrar.php) · [资源控制器章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/controllers.md#resource-controllers)
- `Route::resource` 与 `Route::apiResource` 接通默认动作、HTTP 方法、名称、嵌套、浅层成员和参数别名。支持 `only`、`except`、`names`、`parameters`、`scoped`，以及按动作复用现有路由配置的闭包。控制器依赖继续从当前应用容器解析。缺少动作或配置失败时，已有路由保持可用。
- 真实 HTTP 验证 12 个成功请求、API 剪裁、浅层路径、404/405、PUT 与 PATCH 共用更新动作。固定 PHP `ResourceRegistrar` 的方法、URI、名称、动作和绑定字段与实际 Artisan 输出对照。下一批为单例资源和剩余响应契约，核心账本维持 56/66。
- `59e9f9b46` 隔离快照以 Rust 1.94.0 通过请求输入套件 7 项和路由 HTTP 16 项。实际 `route:list --json --sort=definition` 的 11 条资源路由通过 PHP 对照，`artisan inspire`、严格 Clippy、改动文件格式、三语站点和 18 项浏览器检查通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412` 差异。隔离产物已回收，日志与验收夹具在 `/private/tmp/openrust-04-02-resource.C1OVfDAt`。

## 单例资源控制器

- 对照固定 Laravel 的 `singleton`、`apiSingleton` 与生命周期选项。旧版 `v0.x` 未发现对应的单例注册器。本批复用当前资源控制器的类型化动作、预检、命名、参数和绑定字段。默认查看、编辑、更新，`destroyable` 增加删除，`creatable` 增加创建、保存和删除。 [固定 Laravel 注册器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResourceRegistrar.php#L146) · [单例资源章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/controllers.md#singleton-resource-controllers)
- 嵌套单例保留父参数，子路径使用固定 URI。浅层配置按上游只将编辑、更新、删除移出父路径。显式 `only` 可覆盖两个 API 入口的默认动作集，再沿用生命周期和 `except` 筛选。
- 真实 HTTP 验证单例路径、生命周期、浅层路由与 404／405，定向回归覆盖显式动作选择、命名和父参数绑定字段。实际 Artisan 与固定 PHP 注册器对照。04-02 下一批核对剩余响应契约，核心账本维持 56/66。
- `4bad17635` 隔离快照以 Rust 1.94.0 通过请求输入套件 9 项和路由 HTTP 16 项。实际 `route:list --json --sort=definition` 的 13 条路由通过 PHP 对照，`artisan inspire`、严格 Clippy、改动文件格式、三语站点和 18 项浏览器检查通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412` 差异。隔离产物已回收，证据目录 `/private/tmp/openrust-04-02-singleton.YIt9p9Dj`。

## 流式响应

- 对照固定 Laravel 的 `streamJson`、`eventStream` 和 `streamDownload`，以及 Symfony 7.4.0 的 JSON 生成器与下载处置头。旧版 `v0.x` 未发现对应三种响应构造器，沿用现有 Rust 异步响应体并提取文件系统共用的文件名编码。 [Laravel 流式响应](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#streamed-responses) · [Laravel ResponseFactory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResponseFactory.php)
- PHP 根据生成器的首键区分数组与对象，Rust 由 `StreamedJson::items` 和 `entries` 显式选择，空键值流可生成 `{}`。JSON 使用 Serde 转义。SSE 保留 `update`、多行 `data` 和默认 `</stream>`，可自定义或省略结束事件。Laravel 报告流中异常后停止发送，Rust 将发送后的错误交给 HTTP 响应体，避免发送成功结束事件。流式下载保留可选文件名和处置类型，支持 UTF-8 `filename*`。
- 隔离基线 `22acc2558` 上的真实 HTTP 覆盖逐项 JSON、键值对象、嵌套空数组、HEAD 无正文、SSE 帧与响应头、下载内容与 UTF-8 文件名。受控数据源验证首块在源就绪前发出，另覆盖流中失败、可选结束帧与无效输入。PHP 8.5 调用固定 `ResponseFactory` 与 Symfony 7.4.0 对照上述可比较的 JSON、SSE 帧和下载文件名。下载对照固定 ASCII 回退名以单独核对处置头。
- Rust 1.94.0 请求与响应回归 11 项、本地文件系统 9 项、应用 HTTP 16 项通过。实际 `route:list --json --method=HEAD` 和 `artisan inspire`、工作区严格 Clippy、改动文件格式、三语站点构建和 18 项浏览器检查通过。全仓格式仍仅报告既有的 `sanctum_authentication.rs:1412`。隔离产物已清理，验收日志保留于 `/private/tmp/openrust-04-02-streams.lm10Ah3l`。
- 04-02 仍需核对 `withoutHeader`、`cache.headers`、文件与其他响应工厂及响应宏。Laravel 前端流式消费套件和 Serde/PHP 转义差异仍需按公开契约核对。

## 响应头与缓存控制

- 对照固定 Laravel 的 `ResponseTrait::withoutHeader`、`SetCacheHeaders` 和 Symfony 7.4.0 的 `setCache`、`isNotModified`。旧版 `v0.x` 未找到可复用的响应头移除或缓存中间件。Rust 用类型化 `HeaderName` 及 `withoutHeader`／`withoutHeaders` 表达单个和批量删除，内置 `cache.headers` 复用路由中间件参数解析。 [Laravel 响应头文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#cache-control-middleware) · [Laravel 中间件源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Middleware/SetCacheHeaders.php)
- 成功的 GET 和 HEAD 可合并缓存指令、使用现有或显式验证器，并根据请求转为 304。自动摘要复用锁定版本 `xxhash-rust` 0.8.19 的 XXH3-128，与 PHP `hash('xxh128', ...)` 结果一致。常规流式响应保留逐块发送。日期表达式复用日历解析，日期优先级按固定 Symfony 处理。
- PHP 8.5 在固定源码上验证摘要、缓存头、弱 ETag、时间条件及优先级、POST 和错误响应跳过、流式行为、通配符及显式 ETag。真实 HTTP 另覆盖日期配置、重复条件头、删除多个响应头及带引号的扩展缓存指令。
- 隔离基线 `378d13609` 上使用 Rust 1.94.0 通过请求和响应回归 12 项、应用 HTTP 回归 16 项，实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点和 18 项浏览器检查。改动文件格式通过，全仓格式仍仅报告既有 `sanctum_authentication.rs:1412` 差异。验收日志保留于 `/private/tmp/openrust-04-02-cache-headers.spf4Ki9x`。
- 04-02 保持开放。下一批核对文件与其他响应工厂，后续核对响应宏和流式差异。

## 视图、JSONP、Markdown 与空响应

- 对照固定 `ResponseFactory` 的 `view`、`jsonp`、`markdown`、`noContent` 与 Symfony 7.4.0 的 JSONP 和内容准备。旧版 `v0.x` 的 `NoContent` 属于 HTTP 200 业务响应封装，现有 `Response::empty` 已提供 HTTP 空响应基础。复用当前 Blade 渲染和 `View::first`，以 `Response::view` 显式传入请求上下文、状态和响应头。 [Laravel 响应工厂](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResponseFactory.php) · [Laravel 其他响应类型](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#other-response-types)
- JSONP 校验点分名称、方括号字面量键和保留字，发送 `/**/callback(json);` 与 UTF-8 JavaScript 响应头。回调含控制字符或 Unicode 行分隔符时拒绝。无效回调映射为 `InvalidRequest` 和 HTTP 400，上游抛出 `InvalidArgumentException`。使用 `None` 时沿用普通 JSON 响应。Markdown 发送原文，`noContent()` 对应默认 204，其他状态使用 `empty(status)`。
- PHP 8.5 对照固定代码验证 Unicode 回调与载荷转义、缺省回调、无效回调、Markdown 类型与正文、空响应及视图候选列表与响应元数据。真实 HTTP 验证 Blade 回退模板、变量转义、状态和响应头，并覆盖 HEAD 与无正文状态。
- 隔离基线 `21fa32269` 上使用 Rust 1.94.0 通过请求与响应 13 项、视图 5 项和应用 HTTP 16 项，实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点及 18 项浏览器检查。全仓格式仅报告既有 `sanctum_authentication.rs:1412` 差异。隔离产物已回收，证据目录 `/private/tmp/openrust-04-02-response-factory.xmTozYkc`。
- 04-02 保持开放。下一批核对 `file`／`download` 的二进制文件响应、条件请求、Range 与下载处置头。响应宏及流式差异继续跟踪。

## 二进制文件响应

- 对照固定 Laravel `ResponseFactory::file` 与 `download`，以及 Symfony 7.4.0 `BinaryFileResponse` 的文件元数据、处置头、Range 和 `If-Range`。旧版 `v0.x` 未发现 `If-Range` 实现。本批复用现有文件名编码、缓存指令合并及 Tokio 流式读取。 [Laravel 响应工厂](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResponseFactory.php#L270) · [Symfony 文件响应](https://github.com/symfony/http-foundation/blob/v7.4.0/BinaryFileResponse.php)
- 文件从同一已打开的句柄读取。HTTP Kernel 在响应中间件之后选择区间，因此 `cache.headers` 可以先设置 ETag 或返回 304。默认媒体类型按扩展名判断，调用方可显式设置类型。文件响应提供单个字节区间，忽略畸形和多区间请求并回送完整文件。`If-Range` 使用强 ETag 或与 `Last-Modified` 完全一致的日期。416 明确发送 `Content-Length: 0`。
- 固定 Laravel/PHP 8.5 验证 GET、HEAD、闭合区间、开放区间、后缀区间、304、416、`If-Range` 和下载处置头。对照使用固定媒体类型和 ASCII 文件名回退。真实 HTTP 另覆盖默认媒体类型、UTF-8 回退、空文件、缺失文件和忽略多区间请求。
- 隔离基线 `e370b439d` 上使用 Rust 1.94.0 通过请求与响应 14 项、文件系统 9 项和应用 HTTP 16 项，实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点及 18 项浏览器检查。全仓格式仍仅报告既有 `sanctum_authentication.rs:1412` 差异。隔离产物已回收，证据目录 `/private/tmp/openrust-04-02-binary-file.IsQej8Qt`。
- 04-02 保持开放。后续核对响应宏与流式差异。Symfony 的发送后删文件及可信代理文件转发为独立扩展，当前两个构造器从框架进程直接发送本地文件。

## 响应宏

- 对照固定 Laravel 的响应工厂与 `Macroable`。旧版 `v0.x` 的 URL 对照夹具带有上游 `Macroable`，未找到可复用的 Rust 响应宏。复用当前容器单例、Facade 作用域与 Service Provider `boot`。 [Laravel Response Macros](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#response-macros) · [Laravel Macroable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Macroable/Traits/Macroable.php)
- `Response::r#macro` 在应用作用域内注册异步闭包，同名注册替换。`call` 显式按名称传递 JSON 值参数。`mixin` 接收明确列出的 `ResponseMacro` 和替换开关。`hasMacro` 与 `flushMacros` 只作用于当前应用。Rust 通过具名调用与显式 mixin 清单表达 PHP 动态方法分发及反射。宏执行期间释放注册表锁。
- 固定 Laravel/PHP 8.5 验证注册、动态调用、mixin 替换、清空与未知宏。真实 HTTP 在两个同时运行的应用中验证同名宏隔离、参数错误、mixin 优先级，以及清空后的 500 和其他应用继续服务。
- 隔离基线 `6fb24a3f3` 使用 Rust 1.94.0 通过请求与响应 15 项、应用 HTTP 16 项和容器生命周期 4 项，实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点及 18 项浏览器检查。全仓格式仍仅报告既有 `sanctum_authentication.rs:1412` 差异。隔离产物已回收，证据目录 `/private/tmp/openrust-04-02-response-macros.mCHBYg4O`。
- 04-02 保持开放。下一批核对流式响应的 JSON 转义、事件错误报告与浏览器消费。

## 流式响应与整单元验收

- 对照固定 Laravel 的 `ResponseFactory::stream`、`eventStream`、`Js::encode` 及 Symfony `StreamedJsonResponse`。旧版 `v0.x` 未找到对应流式响应。复用现有 Serde 串流、JSONP 编码器与应用异常处理。`stream` 的原始数据流发送 `X-Accel-Buffering: no`，JSON、下载与磁盘响应维持各自的响应头。 [Laravel 响应工厂](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResponseFactory.php#L135) · [Laravel Js 编码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Js.php#L24) · [Symfony 流式 JSON](https://github.com/symfony/http-foundation/blob/v7.4.0/StreamedJsonResponse.php)
- PHP 8.5 对照验证 HTML 特殊字符、斜杠、Unicode 与代理缓冲响应头。流式 JSON 对键和值统一使用 Symfony 默认字符串转义，SSE JSON 的非 ASCII 默认行为跟随 `Js::encode`。应用中的 SSE 数据源错误由原应用的异常处理器报告，关闭流且省略结束标记。无应用作用域时通过响应体返回错误。Rust 数值和数据模型继续采用 Serde。
- 真实 HTTP 验证转义后的字节、原应用报告一次以及无结束标记；Chromium 用 `Fetch` 在放行第二项前读到第一项，并用 `EventSource` 解析命名 JSON 事件与正常结束标记。浏览器消费验收使用标准 Web API。React、Vue 和 Svelte 的 Laravel 客户端套件保留在应用依赖范围。
- 核对交付计划，JSON、表单、multipart、响应头、流式响应、控制器分派、正文错误与状态码均有真实入口证据。上传保存归 04-09，Windows 实机归 08-04。Rust 保留已有请求正文上限和类型化接口。
- 隔离基线 `64cc3fd4e` 使用 Rust 1.94.0 通过请求与响应 16 项、文件系统 9 项、应用 HTTP 16 项、实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点和 18 项文档浏览器检查。全仓格式仅报告既有 `sanctum_authentication.rs:1412`。隔离产物已回收，证据目录 `/private/tmp/openrust-04-02-stream-parity.4762ezYh`。

## 请求日期读取应用测试时钟

- 对照固定 Laravel `InteractsWithData::date` 和 Carbon 3.8.4。旧版 Calendar 已区分系统与测试时钟，并处理冻结时钟下的缺省字段。沿用其分支及 T-04 应用测试时钟，保留缺省日期、微秒和相对表达式的解析。HTTP 缓存头继续按原系统时钟路径解析。 [固定 Laravel Request::date](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php#L348)
- 在 `2a6bb6ca3` 的干净归档实测。PHP 8.5.12 以固定 Laravel Trait 直接调用 Carbon 3.8.4，输出 22 组日期及格式结果。通过真实 TCP 请求逐项比对 OpenRust 响应，另验证两个应用隔离、显式时区、纽约夏令时切换、作用域恢复、空值和无效输入。
- 请求、日期助手和时间控制定向回归共 24 项通过。Rust 1.94.0 全仓格式、工作区全目标严格 Clippy、独立 `./artisan inspire`、三语站点和 18 项浏览器检查通过。回收隔离编译和文档产物后项目盘可用 100.17 GB（93.29 GiB），提交前周额度剩余 36%。复现日志与 PHP 比对夹具保留于 `/private/tmp/openrust-request-date-clock.vXUAxwQO`。

## HTTP 请求内容协商

- 对照固定 Laravel 13.40 的 Content Negotiation 和 Symfony 7.4.0，复用 `openrust-v0.x` 的有引号参数、数值质量权重、重复条目及格式映射实现和用例。Rust 的候选使用借用切片，`prefers` 用 `Option` 表示无匹配。已构建的请求按需缓存排序，修改请求头后缓存失效，控制器和异常渲染复用同一解析结果。 [Laravel 请求](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#content-negotiation) · [固定实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Concerns/InteractsWithContentTypes.php)
- PHP 8.5 容器用固定 Trait 与 Symfony HeaderBag、AcceptHeader 和 Request 核对 8 组结果。隔离主线 `7379f128a` 的真实 TCP 夹具覆盖质量顺序、带引号分隔符、相同值覆盖、结构化后缀、通配符、重复字段、Markdown 和 AJAX／PJAX；异常测试覆盖 `q=0` 与重复条目的 JSON／HTML 选择。重复 Accept 字段沿用基线的首字段语义。
- 41 项定向回归通过，另用 Redis 8.10.3 复验实际 `down`、`up` 和双进程状态。独立 Artisan `inspire`、全仓格式、全目标严格 Clippy、三语构建及 18 项浏览器检查通过。本批临时容器与编译和站点产物已清理，数据卷清单一致。剩余磁盘 85.03 GB（79.19 GiB），提交前周额度剩余 24%，第二窗口无读数。日志保留于 `/private/tmp/openrust-request-negotiation.WeuqCiLc`。

## 请求头路径与当前URL

- 对照固定 Laravel 的 Request 路径、主机、方法和请求头，以及 Symfony 7.4.0 的查询规范化。复用 `openrust-v0.x` 的 Bearer 提取、查询键排序和 Symfony 黄金用例，地址使用当前可信代理状态。OpenRust 的请求头和 URL 方法用 `Result` 表达无效输入，单头缺失用 `Option` 表达。现有 URI 和签名继续使用原始请求目标。 [Laravel 请求](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#request-path-host-and-method) · [Laravel Request](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php#L117) · [Symfony Request](https://github.com/symfony/http-foundation/blob/v7.4.0/Request.php#L665)
- 在 `feed55226` 隔离源码上真实 TCP 验证编码路径、具名路由、首个请求头值、Bearer 令牌、IPv6 和默认端口、可信代理前缀、查询规范化与增删。PHP 8.5 复验规范化和增删输出。旧版 Symfony 黄金夹具复验 195 个 HTTP URI，其中两项无效 UTF-8 按 Rust 以 400 拒绝。另三项包含 URI 解析器拒绝的原始空白或 NUL。
- 29 项定向回归、真实 Artisan `inspire`、全仓格式、全目标严格 Clippy、三语站点构建、18 项浏览器检查通过。回收专用编译与站点产物后磁盘可用 84.45 GB（78.65 GiB）。提交前周额度剩余 24%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-request-identity.ijSr5kcY`。

## 输入存在性与可变输入

- 对照固定 Laravel 的 Input Presence 和 Merging Additional Input，复用 openrust-v0.x 对输入来源、空白字符串、原始正文、查询回退和文件存在性的实现思路与用例。复用正式版查询、JSON、表单与 multipart 解析器及点路径写入。候选输入整体成功后发布。按实际 HTTP 方法选择查询或表单，JSON 优先。Rust 异步条件回调接收当前请求与输入值，文件通过请求读取。使用 `Option<T>` 表达执行与结果，合法的 `false` 或零保留。 [Laravel Input Presence](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#input-presence) · [Laravel Merging Additional Input](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#merging-additional-input)
- 隔离源码 `f41dcf365` 实际 HTTP 验证 JSON、GET 查询、POST 方法覆盖和 multipart 下的合并与替换，覆盖显式 null、空字符串、false、零、空数组、上传检测和保留、查询回退、非法路径整批回滚、原始正文与 URI。原始固定 Laravel 与 Symfony 7.4.0 在 PHP 8.5 中通过四组行为对照。
- 47 项定向回归、真实 Artisan `inspire`、全仓格式、全目标严格 Clippy、三语站点构建和 18 项浏览器检查通过。回收本批编译与站点产物后磁盘可用 81.49 GB（75.89 GiB）。提交前周额度剩余 24%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-request-mutation.q7n3m5vE`。

## 输入修剪与空字符串规范化

- 对照固定 Laravel 13.40 的 `TrimStrings`、`ConvertEmptyStringsToNull`、`TransformsRequest` 及默认中间件顺序。复用 openrust-v0.x 的 Unicode 修剪、点路径通配和顶层密码例外规则。两层在 CORS 和维护模式之后运行，逐请求累计检查键长。Rust 分别注册字段模式和请求回调，例外保存在应用实例中。只更新解析数据，原始 URI、正文、上传文件保持可读。 [Laravel Requests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#input-trimming-and-normalization) · [Laravel 配置](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/Middleware.php#L510)
- 隔离主线 `668f80912` 真实 TCP 验证查询、JSON、表单、multipart、Unicode 空白、默认密码字段、字段模式和两个独立跳过回调。两个应用并行验证例外隔离，累计键长超限返回 413。PHP 8.5 直接执行固定 Laravel 三组对照，覆盖独立跳过和转换回调读取已修剪的查询值。应用二进制实测 `/up`、畸形 JSON 400 和 SIGTERM，独立 Artisan `inspire` 通过。
- 106 项定向测试报告通过。依环境启用的 MySQL、Redis 和 Mailpit 专项分支本批未运行。全仓格式、全目标严格 Clippy、三语站点构建和 18 项浏览器检查通过。回收专用编译及站点产物后磁盘可用 84.22 GB（78.44 GiB）。提交前周额度剩余 23%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-input-normalization.3cJWlch4`。

## 路径编码和请求正文边界

- 核对固定 Laravel 的 `ValidatePathEncoding` 与 `ValidatePostSize`。沿用 openrust-v0.x 百分号解码后检查 UTF-8 的规则，复用当前 `percent-encoding`。路径检查接入 Rust HTTP 内核的入口，在可信代理处理前执行。内核入口保留应用 API 前缀，异常沿用应用的 JSON／HTML 策略。正文长度检查位于 CORS 和维护模式之后、输入转换之前。Rust 复用既有 8 MiB 限额，Laravel 从 PHP `post_max_size` 读取。 [Laravel ValidatePathEncoding](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Middleware/ValidatePathEncoding.php) · [Laravel ValidatePostSize](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Middleware/ValidatePostSize.php)
- 在隔离主线 `c4000437e` 实际 TCP 验证 Unicode、无效 UTF-8、字面百分号、单次解码、等于限额、超长声明和分块流实际超限。应用二进制实测 `/up`、API 和 Web 畸形路径、CORS 预检优先、超大声明无需发送正文及 SIGTERM。PHP 8.5 使用固定 Laravel 执行路径和声明长度对照。实际 Redis 8.10.3 双进程复验维护模式，独立 Artisan `inspire` 通过。
- 79 项定向回归、全仓格式、工作区全目标严格 Clippy、三语站点和 18 项浏览器检查通过。本批容器、编译与站点产物已回收，Docker 卷清单一致。磁盘可用 84.17 GB（78.38 GiB），提交前周额度剩余 23%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-http-boundaries.Z3CByyMY`。

## 请求旧输入与会话闪存

- 核对固定 Laravel 的 `InteractsWithFlashData` 和 `Store::getOldInput`。复用 openrust-v0.x 的闪存、字段选择和清空思路，以及正式版的请求 `only`／`except` 和会话 `flashInput`。旧输入读取先匹配精确键，再查点路径。Rust 用借用的 `Option` 表达缺失，调用方可提供默认值。上传文件通过独立的类型化文件数据访问，旧输入保存 JSON 可表示的输入。 [Laravel Old Input](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#old-input) · [Laravel 请求实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Concerns/InteractsWithFlashData.php)
- 在隔离主线 `8f49fc70f` 上，用实际 TCP HTTP 与加密会话 Cookie 验证 Array 和 MySQL 8.4.13 的 `flash`、`flashOnly`、`flashExcept` 与 `flush`。覆盖规范化后输入、查询回退、显式 null、字面点键、文件排除、独立会话、一次请求生命周期，以及 flush 后其他会话数据和令牌保持可读。PHP 8.5 运行固定 Laravel 的请求与会话实现，验证两组相同契约。Redis 8.10.3 实际复验会话阻塞和锁。
- 79 项定向回归、真实 Artisan `inspire`、全仓格式、工作区全目标严格 Clippy、三语构建和 18 项浏览器检查通过。本批临时服务与编译、站点产物已清理，Docker 卷清单一致。回收后磁盘可用 84.25 GB（78.46 GiB）。提交前周额度剩余 23%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-old-input.JdYimk3e`。

## 请求 Cookie 与上传文件读取

- 对照固定 Laravel 的 `InteractsWithInput`、`Request::createFromBase` 和 `HttpRequestTest`。复用 openrust-v0.x `6d1a81924` 的 Cookie 存在性、上传分组和直接成员判断用例，以及正式版已有的文件索引与 `data_get`。实际 HTTP 使用 Laravel 的请求转换规则，滤除无文件名的上传和空分组，保留其余数字键及零字节有效文件。 [Laravel 请求读取](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Concerns/InteractsWithInput.php) · [Laravel 请求转换](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php#L543) · [Laravel 请求测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpRequestTest.php)
- Rust 保留现有的单文件 `file()`，用 `fileValue()` 返回类型化的文件或有序分组，用 `allFiles()` 读取整个集合。通配符缺少的成员用 `UploadedFileValue::Null` 表达。Cookie 的命名读取用 `Option` 表达缺失及解密失败，整袋保留 null 值。三语正文同步 Cookies 和 Retrieving Uploaded Files，调整相关小节顺序。 [Laravel Cookies](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#cookies) · [Laravel Retrieving Uploaded Files](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#retrieving-uploaded-files)
- 隔离主线 `397891930` 上用真实 TCP HTTP 验证 Cookie 缺失、空字符串、解密失败与默认值，验证 multipart 分组、空槽、索引、零字节、通配符与直接成员检查。PHP 8.5 运行固定 Laravel 对照两组契约。MySQL 8.4.13 复验 HTTP 数据库验证及会话认证。
- 106 项定向回归、实际 Artisan `inspire`、全仓格式、工作区全目标严格 Clippy、三语站点和 18 项浏览器检查通过。Docker 卷清单一致。回收后磁盘可用 84.25 GB（78.46 GiB），提交前周额度剩余 23%，按用户授权继续。验收日志保留于 `/private/tmp/openrust-request-files.l7NcmxqC`。

## 请求全部输入与文件合并

- 对照固定 Laravel 的 `InteractsWithInput::all`、`keys` 和 `HttpRequestTest`。复用 openrust-v0.x `6d1a81924` 的 `combine`／`merge_files` 合并规则，沿用正式版的上传对象、PHP 键类型、点路径限制和解析缓存。普通输入（含显式 null）优先，双方都是组时保留文件侧新增分支，稳定保留输入键顺序。 [Laravel 请求实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Concerns/InteractsWithInput.php#L66) · [Laravel 请求测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpRequestTest.php) · [Laravel Retrieving All Input Data](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#retrieving-all-input-data)
- Rust 用 `RequestValue` 表示标量、带 `ArrayKey` 的有序组和 `UploadedFile`。用 `allOnly()` 表达 Laravel 的 `all($keys)` 分支，缺失路径保留 null。现有 JSON 输入访问以及上传文件集合保持各自接口，输入回调和筛选接口的类型化对照继续跟踪。
- 隔离主线 `9fc5748cd` 的真实 TCP 验证 multipart 与查询回退、顶层及嵌套重名、null、上传对象、键顺序及重复、JSON 字面点键、整数键和错误 400。PHP 8.5 运行固定 Laravel 的 multipart／JSON 两组对照。
- 57 项定向回归、实际 Artisan `inspire`、全仓格式、工作区全目标严格 Clippy、三语站点和 18 项浏览器检查通过。回收本批独立编译及站点产物后，磁盘可用 84.02 GB（78.25 GiB），提交前周额度剩余 23%，按用户授权继续。验收日志位于 `/private/tmp/openrust-all-input.AtQLs0Vi`。

## 类型化请求回调与字段筛选

- 对照固定 Laravel 的 `InteractsWithData`、`data_get`、`Arr::forget`，复用 openrust-v0.x `6d1a81924` 的类型化选择、路径排除、上传判断与回调用例。正式版存在性和 `mergeIfMissing` 共用 `all()`，消除重复合并。 `only` 支持点路径与通配符，`except` 优先移除字面键且保留剩余数字键。 [Laravel 请求数据接口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Traits/InteractsWithData.php) · [字段筛选](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#retrieving-a-portion-of-the-input-data) · [输入存在性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#input-presence)
- `only`、`except`、`arrayOnly` 和 `collectOnly` 保留 `RequestValue`，`whenHas` 和 `whenFilled` 的参数为 `Option<RequestValue>`。Rust 用 `Option` 表达是否执行回调和由调用方提供的默认结果。 `filled` 只看普通输入，`whenFilled` 执行后读取合并值。会话闪存继续使用普通 JSON 输入，上传对象留在当前请求。
- 隔离主线 `a54250862` 的真实 TCP 验证文件专属字段、混合分组回调、通配符、空值、移除字面点键和稀疏数字键。PHP 8.5 执行固定 Laravel 作对照。Array 和 MySQL 8.4.13 复验旧输入闪存及数据库会话。
- 59 项定向回归、实际 Artisan `inspire`、全仓格式、工作区全目标严格 Clippy、三语站点及 18 项浏览器检查通过。Docker 卷清单一致。回收本批独立编译和站点产物后，磁盘可用 83.98 GB（78.21 GiB），提交前周额度剩余 23%，按用户授权继续。日志位于 `/private/tmp/openrust-request-presence.teIyNf2f`。

## 请求正文参数与 Fluent

- 对照固定 Laravel 的 `InteractsWithInput::post`／`fluent`、`Request::createFromBase` 和 `HttpRequestTest`。复用 openrust-v0.x `6d1a81924` 的顶层正文键读取和 Fluent 缺失／null 回退，当前实现按 Laravel 捕获流程将解码后的 JSON 正文纳入 `post()`，继续复用已缓存的表单、JSON 和标准化输入。 [Laravel 请求读取](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Concerns/InteractsWithInput.php#L112) · [Laravel 请求测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpRequestTest.php) · [Laravel 读取输入值](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#retrieving-an-input-value)
- `fluent()`／`fluentOr()` 读取普通输入，复用既有 JSON `Fluent`。PHP 用 foreach 接收属性并对标量发出警告，Rust 对非法属性形态明确返回 HTTP 400。`fluentOnly()` 用 `Fluent<RequestValue>` 表达带上传对象的只读属性快照。已验证路径读取及独立快照。类型化变更接口和动态属性的路由回退仍需核对。
- 隔离主线 `b3d6ea494` 上用真实 TCP 验证 PATCH 表单、JSON、multipart、查询独立、显式 null、字面点键、合并后正文、错误 400 和上传对象。PHP 8.5 运行固定 Laravel 三组对照。MySQL 8.4.13 复验 Eloquent `AsFluent` 的持久化及类型转换。
- 60 项定向回归、实际 Artisan `inspire`、全仓格式、工作区全目标严格 Clippy、三语站点及 18 项浏览器检查通过。Docker 卷清单一致。回收本批独立编译和站点产物后，磁盘可用 83.92 GB（78.15 GiB），提交前周额度剩余 23%，按用户授权继续。日志位于 `/private/tmp/openrust-post-fluent.WnAKsjtQ`。
