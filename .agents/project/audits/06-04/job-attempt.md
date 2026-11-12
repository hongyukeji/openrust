# 06-04 任务尝试事件

## 基线

对照固定 Laravel 的 [Job Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-events)、[Failed Job Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#failed-job-events)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[任务事件类](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events)和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。复核旧版 v0.x（`6d1a8192`），未发现对应任务尝试事件。

## 实现与 Rust 适配

- 正常完成派发 `JobProcessing → JobProcessed → JobAttempted`。主动释放在 `JobProcessed` 后派发 `JobReleased`。
- 异常重试派发 `JobExceptionOccurred → JobReleasedAfterException → JobAttempted`。永久失败完成失败记录与驱动清理后派发 `JobFailed`，随后派发异常和尝试结束事件。尚未开始执行的尝试不携带耗时。
- 监听器使用可序列化的只读任务快照、异常消息及其源错误链。只有存在匹配监听器或 Fake 时才构造快照。应用异常处理器接收原始类型化错误。执行前监听失败走原重试策略，其余事件监听失败交给应用异常处理器，并保留已完成的队列操作及其结果。
- 超时沿用 Tokio future 取消和既有重试策略，`JobTimedOut` 提供配置超时。Laravel 默认通过进程信号中止 Worker。进程级超时、信号钩子与丢失连接策略仍属于 Worker 后续项。内部链投递与持久化恢复分支继续走各自的交付路径。

## 验收

- SQLite、MySQL 8.4.13、Redis 8.10.3 使用真实 HTTP 派发与独立 Artisan 进程，验证成功、主动释放、异常重试、永久失败、超时、执行前监听器失败及执行后监听器报告。三种驱动均确认第二次尝试和队列清理结果。
- Rust 1.94.0 下运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings`、`cargo test --workspace --locked --offline -- --test-threads=1`。日志合计 575 项通过、6 项忽略，含独立进程夹具。
- 三语示例从实际 `./artisan` 入口在已迁移的 SQLite 空队列运行。有任务时的相同参数由独立 Artisan 集成用例验证。文档构建和 18 项 Chrome 检查通过，覆盖站点根路径和 `/docs/`。

[English](../../../../docs/en/digging-deeper/queues.md#job-attempt-events) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#job-attempt-events) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#job-attempt-events)
