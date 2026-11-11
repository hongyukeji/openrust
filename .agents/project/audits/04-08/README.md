# 04-08 异常报告限流

## 契约与实现

- 对照固定 Laravel 的 `Handler::shouldntReport`、`throttle`、Lottery、Limit 及 Handler 测试中的采样、配额重置、缓存故障回退。参考固定 Goravel 的应用容器和 Boot 入口，从旧版 `6d1a8192:vendor/openrust/framework/src/foundation/exceptions.rs` 提取类型匹配、首个策略生效、固定窗口和限流状态无法使用时的报告放行行为。旧版消费者测试中的回调和默认异常协商继续通过当前 HTTP 与 Artisan 验证。 [Laravel Handler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php) · [Laravel Handler 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/FoundationExceptionsHandlerTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#throttling-reported-exceptions) · [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)
- `Exceptions::throttle::<E>` 支持 Lottery、Limit 和 Unlimited。忽略与已报告实例检查先于限流。准入后执行报告回调和写入日志。存储无法解析或操作失败时继续报告，渲染和退出处理保留现有流程。
- 通过应用容器使用 `cache.limiter` 或默认缓存，复用 04-07 的逐键锁和固定窗口计数，在一次加锁内检查和消耗配额。无显式键时按注册的 Rust 类型计数，显式键可跨类型共用配额。键沿用当前计数器的 SHA-256 命名空间。
- Rust 适配为类型化策略和异步报告。显式调用 `Application::report` 以及实现 `ExceptionHandler` 的调用方需要等待报告，以复用异步缓存并支持当前线程运行时。Rust 策略闭包的 panic 由运行时处理，缓存驱动通过返回错误触发放行。异常报告使用通用缓存计数路径。

## 验收

隔离基线 `21a65e98` 加已验收前置单元和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 432 项测试。工作区测试启用 `OPENRUST_TEST_REDIS_PORT=6379 OPENRUST_REQUIRE_REDIS=1`，包含 04-07 的真实 Redis 跨进程配额回归。
- 7 项异常报告测试覆盖回环 TCP HTTP、Artisan 子进程与显式报告。五个并发请求保持 JSON 500，初始写入三条日志，窗口过期后再写入一条。报告回调次数和日志条数一致。也验证 0/1 与 1/1 采样、相同键跨异常类型共用配额、两个 Array 缓存应用相互隔离、缓存解析失败放行。Artisan 在零配额下跳过日志，保持 stderr 和退出码 1。`./artisan inspire` 正常退出。
- 英文、简体中文、繁体中文的去重和限流示例共六份，以 Rust 1.94.0 编译并运行。VitePress 构建通过，根路径和子路径合计 12 项通用浏览器检查和 6 项错误处理正文导航检查通过。

下一批为 04-09 上传、下载及对象 URL，依赖已验收的 03-01 磁盘与 04-02 HTTP 输入及响应。签名临时 URL 联合验收归 06-01。
