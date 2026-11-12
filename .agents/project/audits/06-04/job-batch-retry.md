# 06-04 Artisan 批次失败任务重试

- 对照固定 Laravel [Batch Failures](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#batch-failures)、[RetryBatchCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryBatchCommand.php)、[RetryCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryCommand.php) 与 [CacheCommandMutex](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/CacheCommandMutex.php)。参考固定 Goravel [QueueRetryCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/console/queue_retry_command.go) 的逐项失败处理。旧版 `v0.x`（`6d1a8192`）的 `queue.rs::retry_failed_inner` 以事务处理重复重试，主线沿用同连接事务思想和现有载荷续发方式。
- `queue:retry-batch` 支持多个批次 ID、重复参数去重、缺失批次与空失败列表诊断。仍可重试的失败任务发回原连接与队列。对于同连接的数据库队列与失败仓库，条件删除和重新入队在同一事务，避免并发重复推送，失败时保留原记录。独立存储保留现有至少一次语义。共享逻辑也服务原有 `queue:retry`，批次失败 ID 由 Worker 在任务成功后清除。
- `--isolated[=退出码]` 使用默认缓存存储的持有者锁，按有序去重后的批次 ID 列表隔离。Rust 哈希化该列表以适配固定长度缓存键。发生错误也尝试释放锁。跨进程隔离依赖共享缓存。
- 基于 `2bd4eb6d` 隔离验收。SQLite 与 MySQL 8.4.13 的实际 HTTP 和独立 Artisan 进程覆盖无参数、缺失、空批次、同一批次并发重试、重复运行、入队失败回滚、缓存锁争用退出码及锁释放。Worker 实际处理重试任务并清除批次失败 ID。
- 三语命令示例从正文抽取，在 SQLite 和 MySQL 的实际 Artisan 进程执行。VitePress 构建、12 项通用浏览器检查及 6 项批次重试小节检查通过。
- Rust 1.94.0 格式、全目标严格 Clippy 和工作区锁定离线串行 572 项通过，6 项辅助进程夹具忽略。现有单任务重试与 Redis Worker 回归通过。
