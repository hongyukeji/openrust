# 07-02 HTTP Client

依赖 01-01。对照固定 [Laravel HTTP Client](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/http-client.md)、[Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Factory.php)、[PendingRequest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php) 和 [HTTP 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpClientTest.php)。装配沿用本项目编译型 Provider，参照固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)。复核旧版 HTTP Client 及服务器回归，沿用 reqwest、超时、流式大小校验、错误脱敏和禁止隐式环境代理的做法。

`Http` 从应用容器取得 `Factory`。每个应用拥有独立连接池、fake 注册、响应序列和请求记录。配置链生成一次性 `PendingRequest`。支持常用 HTTP 方法、JSON、表单、原始正文、multipart、查询参数、Bearer 与 Basic、重定向开关、请求前和响应后回调、状态判断、有限重试、fake、测试断言及并发池。HTTP 4xx、5xx 由调用方决定是否转换成错误，重试耗尽的行为可配置。并发池的每项结果独立。顺序契约见下文的并发池与批次。

Rust 使用 `Duration`、类型化 JSON 及分开的配置方法表达 PHP 可选参数。响应缓冲复用字节存储，默认连接选项复用 reqwest 连接池，自定义连接超时或重定向选项创建独立客户端。重试前校验请求是否可重放，一次性流在启用重试时返回明确错误。请求记录可观察已缓冲的正文，流式上传的正文不可回读。显式查询项覆盖 URL 查询串，编码使用 RFC 3986。原始 `reqwest::Request` 可由请求前回调调整。Laravel 的 Guzzle 专用选项、动态宏及 Digest Auth 仍单独跟踪。旧版目标白名单、DNS 固定和熔断属于额外传输能力，当前范围未接入这些能力。

## 验收

- Rust 1.94.0，`cargo fmt --all --check`、`cargo test --workspace --locked --offline` 和 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。工作区 500 项通过，3 项为集成测试启动的子进程入口。
- 五项定向测试覆盖实际 HTTP JSON、表单、multipart、Raw、认证、查询编码、重定向、状态处理、超时、响应大小、重试成功及耗尽、fake 拦截与记录、并发池和两个实际 HTTP 应用间的 Facade 隔离。
- 独立启动验收使用临时 SQLite，Artisan `migrate --force` 和 `inspire` 退出 0，`GET /up` 和 `GET /` 返回 200。
- 英文、简体、繁体文档的三份 Rust 示例实际运行。VitePress 构建、12 项通用浏览器检查及 6 项 HTTP Client 页面内导航通过。
- 按隔离副本验证后，独立核对与在途 gRPC 差异合并的工作区锁文件。锁定离线 metadata 保留 reqwest、tonic 和 tokio-stream 的解析。

下一单元 07-03 Processes，前置 01-01 已提交。

## 批次回调与请求事件

对照固定 Laravel 的 [Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Batch.php) 和 [HTTP Client 事件](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Events)。Rust 批次沿用具名请求池，配置回调持有只读计数快照，`send` 消费批次。五个回调按注册和完成阶段运行，返回值保留每项错误和注册顺序。Rust `Result` 显式传递错误，应用可以区分 HTTP 状态和客户端错误。延后执行见下文的并发池与批次。

应用级工厂接入自己的事件派发器，按请求尝试派发 `RequestSending`、`ResponseReceived` 和 `ConnectionFailed`，沿用公开类型名称。事件用可克隆的请求快照承载已缓冲内容，连接异常区分超时与其他连接失败。连接失败事件发生于请求尝试阶段，HTTP 4xx、5xx 仍派发已收到响应事件。

七项 HTTP 定向回归通过，其中批次验证成功、HTTP 错误、未匹配 fake、失败时跳过 then、finally 及结果顺序。真实回环 HTTP 验证请求事件、成功响应和超时对应的类型化载荷。三语站点构建及两个相对部署路径的十二项通用浏览器检查通过。

以已提交主线 `cd17b297e` 创建隔离验收副本。Rust 1.94.0 全目标严格 Clippy 和本批文件格式检查通过。全仓格式检查只报告既有 Sanctum 断言换行。三语邮件和 HTTP Client 页面在 `/` 与 `/docs/` 下新增章节检查共十二项通过。

## 并发池与批次

- 固定 Laravel 的有界及默认池按完成顺序收集结果，显式 null 模式按注册顺序读取已启动的请求。Rust 使用非零并发值，`NonZeroUsize::MAX` 表示不限并发，`poolOrdered` 表示全量启动后按注册顺序返回。池和批次支持命名与匿名注册，数字键用字符串承载。两种池入口都保留单项 `Result`。旧版 HTTP Client 提供传输和重试回归，当前并发逻辑沿用 Rust 请求池。 [固定 Pool](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Pool.php)
- 批次在 `before` 后启动请求，按完成顺序派发 `progress`／`catch`，以注册顺序传递最终结果。2xx 进入 `progress`，3xx 只更新已完成计数，4xx／5xx 或 Rust 单项错误进入 `catch`。无失败时执行 `then`，正常完成时执行 `finally`。空批次验证了三段回调。`send` 及 `defer` 消费批次，延后执行复用 HTTP／Artisan／队列生命周期。 [固定 Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Batch.php)
- Rust 1.94.0 锁定离线运行 9 项 HTTP Client 定向用例和工作区严格 Clippy。变更文件格式通过，工作区仍有基线 Sanctum 排版差异。实际 Artisan 和回环 HTTP 运行三语共用例，另核对两种生命周期中的延后批次抵达真实上游。三语 VitePress 构建及 12 项桌面和移动浏览器验收通过。07-02 继续核对事件与剩余契约。

## 请求事件与传输边界

- 核对固定 Laravel 三种事件的公开载荷及 Guzzle 处理栈。应用工厂使用所属派发器，独立工厂可通过 `withDispatcher` 注入派发器。请求快照包含方法、URL、标头、可读取正文和 `withAttributes` 元数据，后者不进入网络请求。连接失败采用可匹配的连接／超时错误。 [固定事件定义](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Events)
- `RequestSending` 在每次请求和重定向跳转的 `beforeSending` 回调前派发。最终 `ResponseReceived` 在 `afterResponse` 前派发，含 4xx、5xx。请求级重试重新派发事件。连接或超时异常使用实际尝试发送的请求快照。重定向上限与缓冲大小限制为独立错误。真实回归核对跨源跳转清除上一个源的授权头、大型跳转正文和最终响应大小限制。
- Rust 1.94.0 锁定离线执行 10 项 HTTP Client 定向用例，工作区全目标严格 Clippy 与本批文件格式通过。真实 Artisan、回环 HTTP 及本地上游验证 302、404、连接关闭和事件载荷。三语 VitePress 构建和 12 项浏览器检查通过。
- Digest Auth 已由固定 Laravel 源码确认，仍需以实际挑战和响应验收。PHP 层的动态宏与 Guzzle 选项适配继续单独记录。
