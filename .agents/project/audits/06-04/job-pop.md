# 06-04 任务预约事件

## 基线

对照固定 Laravel 的 [Job Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-events)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[JobPopping](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/JobPopping.php)、[JobPopped](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/JobPopped.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。复核旧版 v0.x（`6d1a8192`）的预约与超时重试用例，未发现这两个事件。

## 实现与 Rust 适配

- 实际轮询前派发 `JobPopping`，成功预约后、执行前派发 `JobPopped`。守护和单次运行共用事件顺序。Rust 以只读预约快照传递驱动任务 ID、队列、尝试次数和完整载荷。只有匹配的监听器或事件 Fake 观察时才复制载荷。
- 预约前监听器失败按命令错误退出。预约后监听器失败由当前应用的异常处理器报告，Worker 等待一秒并保留预约。驱动在预约到期后按原有尝试计数重取，避免提前交给另一个 Worker。此批保持现有驱动错误策略。

## 验收

- SQLite、MySQL 8.4.13 和 Redis 8.10.3 实际 HTTP 派发与独立 Artisan 验证空队列、事件顺序、预约前失败、预约后报告、租约内不可提前重取、到期后第二次尝试成功以及无真实监听器时的 Event Fake。
- 三语空队列 `./artisan` 示例实跑。Rust 1.94.0 格式、严格 Clippy 与锁定离线串行工作区通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。
- 三语文档构建和 18 项 Chrome 检查覆盖 `/` 和 `/docs/` 的预约章节。

[English](../../../../docs/en/digging-deeper/queues.md#job-reservation-events) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#job-reservation-events) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#job-reservation-events)
