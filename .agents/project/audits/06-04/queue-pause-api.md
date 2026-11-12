# 06-04 编程式队列暂停与事件

## 基线

对照固定 Laravel 的 [Pausing and Resuming Queue Workers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#pausing-and-resuming-queue-workers)、[QueueManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/QueueManager.php)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 及 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel 队列契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型登记。旧版 v0.x（`6d1a8192`）未提供相应的队列暂停控制及事件，沿用其独立进程 Worker 验证方法。

## 实现

- Artisan 与应用通过同一个 QueueManager 修改暂停状态。Queue 门面提供永久、限时及全局暂停和恢复，并提供单队列及多队列查询。限时暂停沿用缓存的时长与截止时间类型。
- QueueManager 在缓存修改成功后向所属应用派发事件。Worker 在预留任务前比较本次与上次观察到的暂停队列。事件载荷使用可序列化的 Rust 类型，供现有事件系统登记。
- 缓存接口按队列读取状态。Worker 命令的事件状态输出和终止原因属于后续续批。

## 验收

- 真实 HTTP 和独立 Artisan 在 SQLite、MySQL 8.4.13、Redis 8.10.3 验证限时暂停到期、Worker 观察事件、管理事件、全局状态和普通队列消费。双应用验证绝对截止时间、缓存和事件隔离。
- 三语示例的函数主体逐项与 HTTP 夹具核对并执行。VitePress 构建及 18 项 Chrome 检查通过，覆盖 `/` 与 `/docs/`。
- Rust 1.94.0 格式检查及锁定离线全目标严格 Clippy 通过。串行工作区测试日志合计 582 项通过、6 项忽略，含独立 Artisan 子进程的夹具报告。

[English](../../../../docs/en/digging-deeper/queues.md#pausing-queues-in-code) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#pausing-queues-in-code) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#pausing-queues-in-code)
