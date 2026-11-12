# 06-04 任务批次派发与 Worker 记账

- 核对固定 Laravel [Job Batching](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-batching)、[PendingBatch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/PendingBatch.php)、[Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) 与 [CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php)。参考固定 Goravel [队列契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go) 的编译型登记。旧版 `v0.x`（`6d1a8192`）无批次派发实现，沿用其 Worker 验收路径和主线的独立 Artisan 进程。
- `Bus::batch`、`findBatch` 和 `PendingBatch` 使用应用作用域。以已有 `Payload` 表达任务，保存批次 ID。SQL 仓库与数据库队列共享一笔写事务，拒绝跨连接或非数据库驱动，避免部分入队。应用使用 `queue.batching` 的连接和表配置。
- Worker 在成功确认、最终失败和重投递恢复时更新批次。追加 `job_batches_jobs` 持久化终态，由同一事务写入终态和批次计数。一次投递只记一次失败，同一任务只记一次成功。任务业务操作仍遵循至少一次执行语义。默认首次最终失败取消批次。中间件 `SkipIfBatchCancelled` 使应用可声明跳过已取消批次的任务。失败重试成功时扣减待处理数、移除失败 ID，并保留累计失败次数。
- 验收以 `5dc96709` 为隔离基线。真实 HTTP、实际 Artisan 迁移和独立 Worker 在 SQLite 与 MySQL 8.4.13 通过。测试覆盖并发 Worker、普通重试、手工重试、成功载荷重投递、默认取消、允许失败、取消后中间件跳过，以及第二条入队语句失败时首条任务与批次记录一起回滚。
- Redis 等跨存储批次派发、批次回调、清理命令、链式任务与 `make:job --batched` 模板仍待补齐。
- 三语派发示例均抽取、编译，并在两库真实 HTTP 和 Artisan Worker 路径执行。站点构建、12 项通用浏览器检查与 6 项批次小节检查通过。
- Rust 1.94.0 工作区 572 项锁定离线串行测试通过，6 项辅助进程夹具忽略。格式与全目标严格 Clippy 通过。现有 Redis 队列回归通过。
