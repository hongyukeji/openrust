# 06-04 同步队列生命周期事件

## 基线

对照固定 Laravel [Job Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-events)、[SyncQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SyncQueue.php)、[SyncJob](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/SyncJob.php)、[Job::fail](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/Job.php) 与 [QueueSyncQueueTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueSyncQueueTest.php)。参考固定 [Goravel Sync 驱动](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/driver_sync.go) 的就地执行和链推进。v0.x 未找到对应的同步生命周期。复用当前任务快照、失败回调和事件分发。

## 实现与 Rust 适配

- 同步驱动在当前应用作用域中发出处理前、处理后和尝试结束事件。异常先发出异常事件，再运行失败回调并发出失败事件，最后发出尝试事件。处理前监听失败时无测量耗时，尝试事件保留原始异常。
- 快照使用 Laravel 的空驱动 ID、sync 队列和一次尝试。实际处理上下文携带调用选择的队列及事务。与 Worker 共享毫秒取整和异常源链格式。仅在存在观察者时构造事件快照。

## 验收

- 独立 Artisan 与真实 HTTP 夹具在 SQLite、MySQL、Redis 环境中验证同步成功、任务失败、失败回调错误、处理前与处理后监听错误。覆盖事件顺序、原始错误、同步任务快照及耗时。处理前拒绝时实际处理器未执行。
- 三语示例实际启动应用并运行同步任务，验证注册的处理后回调。文档构建及 18 项浏览器检查通过。Rust 1.94.0 格式和严格 Clippy 通过。锁定离线最终串行工作区验收通过，含子进程 577 项通过、7 项忽略。

[English](../../../../docs/en/digging-deeper/queues.md#synchronous-job-events) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#synchronous-job-events) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#synchronous-job-events)
