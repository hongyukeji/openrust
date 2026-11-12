# 06-04 Worker 状态输出和退出原因

## 基线

对照固定 Laravel 的 [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[WorkCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/WorkCommand.php)、[WorkerStopReason](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/WorkerStopReason.php)、[WorkerStopping](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/WorkerStopping.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel 队列契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型 Worker。复核旧版 v0.x（`6d1a8192`）的 Worker 循环、失败和关闭用例，未发现相同的停止事件及 JSON 行输出。

## 实现与差异

- Worker 返回原有的已处理任务计数，并向所属应用派发 `WorkerStopping`。现有正常停止条件分别标记为队列空、任务上限、时间上限、信号和重启信号。
- Artisan 随运行输出任务开始、成功、失败和异常释放，以及暂停、恢复和停止。手动释放是成功执行且预约已释放。命令作用域内观察状态，避免在应用事件注册表中留下跨命令监听器。终端逐行刷新。Redis 阻塞预约继续由队列自己的等待窗口和外层取消控制，避免客户端默认 500 毫秒响应超时早于阻塞窗口。应用内调用沿用其 Output 缓冲。JSON 时间戳采用应用配置的时区。静默标志抑制正常输出。
- Rust 使用 `QueueError` 和其显示消息报告任务失败。内存用量字段暂为 null，终端使用普通文本，单次执行省略停止摘要。内存上限、失联和超时导致的 Worker 停止策略，以及其余 Worker 参数另行核对。

## 验收

- 真实 HTTP 派发及独立 Artisan 在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 验证成功、失败、异常重试和手动释放，暂停、恢复、五种退出原因、正常退出码、任务数、静默及时间戳。应用监听器实测收到重启事件。
- 三语章节的 `./artisan` 示例分别实跑，另验收 `--silent`。Rust 1.94.0 格式、全目标严格 Clippy 与锁定离线串行工作区测试通过。测试日志合计 575 项通过、6 项忽略，含独立进程夹具。
- VitePress 构建与 18 项 Chrome 检查覆盖 `/`、`/docs/` 及三语状态章节。

[English](../../../../docs/en/digging-deeper/queues.md#worker-status-and-exit-reasons) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#worker-status-and-exit-reasons) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#worker-status-and-exit-reasons)
