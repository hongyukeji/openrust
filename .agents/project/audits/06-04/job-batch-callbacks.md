# 06-04 任务批次生命周期回调

- 对照固定 Laravel [PendingBatch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/PendingBatch.php)、[Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) 与 [Dispatching Batches](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#dispatching-batches)。固定 Goravel [Queue](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/queue) 的编译型登记作为参考。旧版 `v0.x`（`6d1a8192`）无批次回调实现，复用主线的应用作用域、终态事务和 Worker 进程验收。
- `before`、`progress`、`then`、`catch`、`finally` 和 `onFailure` 接收稳定的回调名，存入批次选项。Rust 通过 Provider 注册函数指针，由应用和 Worker 各自在自己的容器解析。Laravel 的 `allowFailures(callback)` 由 Rust 的 `onFailure(callback)` 表达，并启用允许失败。失败参数使用持久化错误消息。
- `before` 收到带 ID 的初始快照，在写事务前执行，使回调可独立访问数据库，同时保留账本与入队的原子提交。Worker 回调在终态提交后执行并使用独立应用作用域。最终失败触发首次 `catch`，允许失败时触发 `progress` 和 `onFailure`。`finally` 以仍待处理且已失败的任务 ID 集判定首次全部有终态，避免重复失败历史造成提前触发。终态重放不会重放回调。回调错误记录到应用日志。进程在提交到执行回调之间退出时，该次回调可能丢失。
- 基于 `987febd9` 的隔离验收。SQLite 与 MySQL 8.4.13 的真实 HTTP 和独立 Artisan 进程验证成功、失败、取消、首次终态、重试、重投递、未知回调和回调报错后继续。仍待接入 Laravel 批次生命周期事件和 Redis 跨存储派发。
- 三语回调示例从文档抽取，注册到真实 HTTP 应用和独立 Artisan Worker。两库确认回调输出带正确批次 ID。VitePress 构建、12 项通用浏览器检查及 6 项回调小节检查通过。
- Rust 1.94.0 格式、全目标严格 Clippy 和工作区锁定离线串行 572 项通过，6 项辅助进程夹具忽略。
