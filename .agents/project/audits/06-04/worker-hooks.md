# 06-04 Queue 生命周期监听入口

## 基线

对照固定 Laravel [Job Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-events)、[Failed Job Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#failed-job-events)、[QueueManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/QueueManager.php)、[QueueManagerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueManagerTest.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。固定 Goravel [Worker](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/worker.go) 及 v0.x 未找到对应的一组公开监听方法。复用现有应用事件分发与 Worker 事件。

## 实现与 Rust 适配

- Queue 门面和 QueueManager 提供 `before`、`after`、`exceptionOccurred`、`looping`、`failing`、`starting` 和 `stopping`，通过当前应用分发器注册。回调采用具体事件类型与现有异步监听结果。
- `whenJobRunsLongerThan` 监听已测量的任务尝试。耗时严格大于阈值时调用回调，传入原任务快照和耗时。阈值与回调耗时采用 Rust Duration，现有事件毫秒字段用于转换。
- `looping` 沿用 Worker 既有轮询否决规则。此批交付注册入口，同步队列的生命周期事件另按上游契约核对。

## 验收

- 复用 SQLite、MySQL、Redis 的实际 HTTP 和独立 Artisan Worker 夹具，使用门面入口验证生命周期、轮询否决、失败和长任务。补无耗时、小于、等于、大于阈值以及双应用隔离。
- 三语示例通过应用 Provider 实际注册并触发失败回调。Rust 1.94.0 格式、严格 Clippy 和锁定离线串行工作区通过，含子进程 577 项通过、7 项忽略。文档构建及 18 项浏览器检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#registering-worker-callbacks) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#registering-worker-callbacks) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#registering-worker-callbacks)
