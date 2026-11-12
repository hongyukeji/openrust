# 06-04 Worker 任务间隔与空闲退出

## 基线

对照固定 Laravel 的 [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[WorkCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/WorkCommand.php)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel 队列契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型 Worker。复核旧版 v0.x（`6d1a8192`）的 Worker 循环和退出用例，未发现同名空闲退出参数。

## 实现与 Rust 适配

- `--rest` 在任务执行后等待，`--stop-when-empty-for` 在没有可取任务且达到指定空闲时长时以 `empty_for` 退出。空闲时钟从启动开始并在每次任务结束后重置。立即空队列退出优先，单次 Worker 沿用单次执行语义。
- Tokio 单调时钟限制休息和空闲等待。Worker 在空闲期限到达后仍先尝试预约，再根据空队列结果决定是否退出。Redis 阻塞预约采用配置值和剩余空闲窗口中较短的期限，同时可由新任务唤醒。非阻塞驱动沿用即时预约。两个参数由 Artisan 的运行配置传递，既有 WorkerOptions 的公开结构保持兼容。

## 验收

- SQLite、MySQL 8.4.13 和 Redis 8.10.3 实测 HTTP 派发与独立 Artisan，覆盖至少一秒的任务间隔、最后一个任务完成后重新计满两秒、事件计数、立即空队列优先、零值禁用及 Redis 阻塞期间有新任务提前唤醒。
- 三语 `./artisan` 示例实跑返回 `empty_for`，冷构建后重跑的计时为 2.28–2.29 秒。Rust 1.94.0 格式、严格 Clippy 与锁定离线串行工作区测试通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。
- 三语状态章节及示例在 VitePress 构建和 18 项 Chrome 检查中通过，覆盖 `/` 与 `/docs/`。

[English](../../../../docs/en/digging-deeper/queues.md#worker-rest-and-idle-timeout) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#worker-rest-and-idle-timeout) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#worker-rest-and-idle-timeout)
