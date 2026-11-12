# 06-04 任务批次生命周期事件

- 对照固定 Laravel [PendingBatch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/PendingBatch.php)、[Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) 和 [Bus Events](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Events)。固定 Goravel [Event Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/application.go) 作为事件登记参考。旧版 `v0.x` 无批次生命周期事件，沿用其事件派发思路及主线 06-02 Dispatcher。
- 新增 `BatchDispatched`、`BatchStarted`、`BatchFinished`、`BatchCanceled`。事件在相关事务提交后派发，携带批次快照，自动取消事件携带持久化失败消息。事件载荷单独保留失败任务 ID 供序列化，批次 API JSON 保持原契约。
- 同一 Worker 转换中，开始事件先于相关回调，完成事件位于进度回调和 `then` 之间。批次终态重放与重复显式取消均不重复发事件。多个 Worker 的监听器可能并发执行。监听器返回错误时记录日志，已提交的状态继续有效。提交后到事件派发前的进程退出可能丢失该次事件。
- 基于 `92e14e7f` 隔离验收。SQLite 与 MySQL 8.4.13 的实际 HTTP 与独立 Artisan Worker 验证四种事件、回调顺序、失败原因、显式取消、终态重投递、取消事件序列化和三语监听器示例。
- 三语示例从正文抽取，分别在两库的独立 Worker 监听到正确的批次 ID。VitePress 构建、12 项通用浏览器检查和 6 项批次事件小节检查通过。Rust 1.94.0 格式、全目标严格 Clippy 和工作区锁定离线串行 572 项通过，6 项辅助进程夹具忽略。
