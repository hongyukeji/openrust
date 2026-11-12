# 06-04 Worker 超时退出

## 基线

核对固定 Laravel [Job Expirations and Timeouts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-expirations-and-timeouts)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[WorkerStopReason](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/WorkerStopReason.php) 及 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。v0.x（`6d1a8192`）未发现对应的守护进程退出机制。

## 实现与 Rust 适配

- 守护进程实际超时后，沿用 `Interruptible` 通知，处理终态失败，再派发 `JobTimedOut` 并以 `timed_out` 退出。可重试预约保留到租约到期。终止路径跳过常规异常释放和尝试结束事件。
- 默认退出码 1。应用可设置 `QUEUE_WORKER_TIMED_OUT_EXIT_CODE` 为 0 至 255，或以 `QUEUE_WORKER_KILL_ON_TIMEOUT=false` 继续消费并使用常规重试。单次执行保持现有超时处理。
- Laravel 以进程信号触发强制退出。Rust 用 Tokio 计时取消异步 future，再返回 Artisan 退出码。同步阻塞任务的强制终止仍需单独实现和验收。

## 验收

- SQLite、MySQL 8.4.13 和 Redis 8.10.3 使用真实 HTTP 派发与独立 Artisan 进程，验证默认退出码 1、配置退出码 124、失败入库、超时事件、下一任务未提前消费、原租约到期后第二次尝试、关闭自动退出后继续消费及单次模式回归。通过进程状态与 JSON 双重核对退出码。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和串行工作区测试通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。三语 Artisan 命令实跑，文档构建和 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#job-expirations-and-timeouts) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#job-expirations-and-timeouts) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#job-expirations-and-timeouts)
