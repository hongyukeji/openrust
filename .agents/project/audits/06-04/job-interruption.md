# 06-04 正在运行任务的中断通知

## 基线

对照固定 Laravel 的 [Reacting to Worker Signals](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#reacting-to-worker-signals)、[Interruptible](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Queue/Interruptible.php)、[JobInterrupted](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/JobInterrupted.php)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。复核 v0.x（`6d1a8192`），未发现对应契约和事件。

## 实现与 Rust 适配

- 注册的 JobHandler 通过 `interruptible()` 提供可选能力。Registry 在中间件链开始时登记当前 Handler，在中间件链结束时解除。中断回调在该次尝试的应用和日志作用域执行。
- 退出信号先派发 `WorkerInterrupted`，再通知正在执行的任务。回调成功后派发 `JobInterrupted`。失败交给应用异常处理器，Worker 继续原有收尾。
- Rust 沿用 Tokio 计时。实际超时先取消异步 future，再用 SIGALRM 对应的编号通知已登记的 Handler。Windows 使用兼容编号 14。原有超时失败和重试策略随后执行。事件使用只读快照，启用相应监听器或 Fake 时才构造快照。进程级超时退出与 Windows Ctrl+C 实机仍待单独验收。
- 中断选择分支增大 Worker future 后，链式批次在调试构建中触发线程栈溢出。仅链式批次执行的 future 改为装箱，普通任务的现有调用路径保持。

## 验收

- SQLite、MySQL 8.4.13 和 Redis 8.10.3 通过真实 HTTP 派发及独立 Artisan 进程验证 SIGTERM、超时通知、回调成功后派发事件、回调报错时报告并继续记录超时失败。检查回调能够读取本次尝试的日志上下文。
- 原先触发栈溢出的 SQLite、MySQL 链式批次用例已复跑通过，包含真实 Redis 队列。Rust 1.94.0 格式、锁定离线严格 Clippy 和串行工作区测试通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。
- 从三语正文提取 Rust 示例并编译运行。文档构建和 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#notifying-a-running-job) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#notifying-a-running-job) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#notifying-a-running-job)
