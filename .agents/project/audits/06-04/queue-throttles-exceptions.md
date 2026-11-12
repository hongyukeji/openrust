# 06-04 队列异常节流

## 基线

核对固定 Laravel [Throttling Exceptions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#throttling-exceptions)、[ThrottlesExceptions](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/ThrottlesExceptions.php)、[ThrottlesExceptionsWithRedis](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/ThrottlesExceptionsWithRedis.php) 与 [DurationLimiter](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Redis/Limiters/DurationLimiter.php)。对照 [缓存集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/ThrottlesExceptionsTest.php) 与 [Redis 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/ThrottlesExceptionsWithRedisTest.php) 的阈值、清零、回退和终态。运行方式参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。v0.x（`6d1a8192`）有应用异常报告节流，未找到队列异常节流中间件，当前实现复用本分支的缓存限流器、Redis 原子计数和 Worker 失败结算。

## 实现与 Rust 适配

- 匹配的失败消耗额度，成功清零。阈值后跳过处理器并延迟重试，缓存版本使用窗口剩余时间加三秒，Redis 版本使用窗口剩余时间。支持按任务类型、共享键和任务 UUID 分组。
- `when` 和 `reportWhen` 接收类型化异常及限流器上下文。Redis 回调读取本次预约检查时的剩余值与到期时间。Rust 以只读快照表示这些数据，缓存版本可访问类型化限流服务。以 `backoffWhen` 表示 Laravel 的回调退避，以谓词表示异常类型判断。
- `deleteWhen` 优先于 `failWhen`。失败由 Worker 在保留预约的进程内完成，跨进程任务通过回传终态信号维持同一结算路径。缓存限流和 Redis 键沿用现有隔离命名，Redis 存储格式为 OpenRust 固定窗口计数。

## 验收

- 通过真实 HTTP 派发与独立 Artisan 在 SQLite、MySQL 8.4.13、Redis 8.10.3 上验收阈值、成功清零、窗口到期后执行、分钟退避、异常报告、条件放行、删除与终态失败。Redis 变体额外核对回调可见的剩余额度和到期时间。
- 三种预约后端均验证隔离任务触发 `failWhen` 后由父 Worker 写入失败记录并移除预约。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和工作区串行测试通过。三语示例提取后使用实际 sync 队列执行，文档构建与 18 项 Chrome 检查通过。 首轮全量回归触发既有会话测试对随机 Cookie 子串的偶发断言失败，单项复跑通过。

[English](../../../../docs/en/digging-deeper/queues.md#throttling-exceptions) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#throttling-exceptions) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#throttling-exceptions)
