# 06-04 Worker 内存限制

## 基线

核对固定 Laravel [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[WorkCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/WorkCommand.php)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[WorkerStopReason](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/WorkerStopReason.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。旧版 v0.x（`6d1a8192`）未找到对应内存限制或退出码测试。

## 实现与 Rust 适配

- `queue:work --memory` 默认 128 MiB。零或负数关闭上限检查。单次模式跳过检查。守护进程在任务或轮询边界检查，等于上限时同样退出。内存退出返回状态 12、`MaxMemoryExceeded` 及 JSON 原因 `memory`。停止事件携带 MiB 读数，JSON 采用一位小数和 `warning` 级别。
- Laravel 以 PHP 分配器占用和 PHP `memory_limit` 为基准。Rust 以进程常驻内存为基准，Linux 使用低开销 `/proc/self/statm` 读数。百分比以 `queue.memory_limit` 为基准，根应用通过 `QUEUE_WORKER_MEMORY_LIMIT` 配置 MiB。内存限制用于决定何时停止 Worker，进程硬上限由部署环境控制。
- 编程入口 `Worker::daemon` 继续返回处理计数，Artisan 返回实际停止码。默认检查覆盖 macOS、Linux 和 Windows 的内存读取 API，完整 Worker 的运行结果按验收平台记录。

## 验收

- macOS 上用真实 HTTP 派发及独立 Artisan 进程，在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 验证退出码、停止事件、JSON、只处理首个任务并保留下一任务、单次执行、百分比、非法值和禁用限制。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和串行工作区测试通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。
- 从三语文档提取并运行六条 `./artisan` 示例。另以 1 MiB 阈值实测 shell 退出码 12，JSON 原因为 `memory`。文档构建和 18 项 Chrome 检查通过。
- Linux 容器的独立 RSS 探针测得分配前后 1,720,320 与 18,497,536 字节。Linux 完整 Worker、Windows 实机和 Laravel 的自定义内存退出码接口仍待单独验收。

[English](../../../../docs/en/digging-deeper/queues.md#worker-memory-limit) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#worker-memory-limit) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#worker-memory-limit)
