# 06-04 Worker 生命周期与循环控制

## 基线

对照固定 Laravel 的 [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[WorkerStarting](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/WorkerStarting.php)、[WorkerIdle](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/WorkerIdle.php)、[Looping](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/Looping.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。复核旧版 v0.x（`6d1a8192`）的 Worker 循环与退出用例，未发现同名生命周期及否决契约。

## 实现与 Rust 适配

- 守护 Worker 发出 `WorkerStarting`、`Looping`、`WorkerIdle` 与既有 `WorkerStopping`。单次运行使用单次事件边界。实际无可用任务的轮询才发出空闲事件。
- `Looping` 沿用应用事件分发器的 `until`，以同步监听器的首个响应决定是否否决当前轮询。只有 Rust 布尔 `false` 否决。否决期间仍可响应终止信号和退出时限。零秒 sleep 在否决时采用一秒退避，避免忙等。
- 守护事件携带连接、队列和现有 WorkerOptions 快照。维护模式和任务预约前后事件保留为后续交付。

## 验收

- SQLite、MySQL 8.4.13 和 Redis 8.10.3 实测 HTTP 派发及独立 Artisan。验证否决后重试、首个响应短路、否决期间到达上限且保留任务、否决等待期间重启优先于空队列退出、单次运行消费任务且跳过守护事件，以及真实空轮询发出空闲事件。
- 三语 `./artisan` 示例实跑返回 `empty`，另验证 `--silent`。Rust 1.94.0 格式、严格 Clippy 与锁定离线串行工作区测试通过，最终日志合计 575 项通过、6 项忽略，含独立进程夹具。
- VitePress 构建与 18 项 Chrome 检查覆盖 `/` 和 `/docs/` 的三语生命周期章节。

[English](../../../../docs/en/digging-deeper/queues.md#worker-lifecycle-events) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#worker-lifecycle-events) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#worker-lifecycle-events)
