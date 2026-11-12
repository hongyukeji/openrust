# 06-04 向已派发批次追加任务

- 对照固定 Laravel [Adding Jobs to Batches](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#adding-jobs-to-batches)、[Batch::add](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) 和 [DatabaseBatchRepository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/DatabaseBatchRepository.php)。参考固定 Goravel [Queue 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go) 的编译型任务边界。旧版 `v0.x` 无任务批次追加实现，复用主线的账本、事件和队列。
- `Batch::add` 从应用作用域读取最新批次选项，增加总数和待处理数，并清除完成时间。已有取消状态和失败信息保留。Rust 返回更新后的快照，批次缺失返回 `None`，空追加只读取快照。每个追加任务分配新的队列 UUID，复用同一载荷时每次仍独立记账。
- 数据库队列与计数写入同一事务。Redis 追加记录使用独立发布 ID 和增量表，确认初始发布后才允许追加发布。原始待发布表和标记继续使用原标识，可跨迁移恢复。追加沿用恢复命令且不重复发送 `BatchDispatched`。链式批次任务待后续接入。
- 基于 `bc72ab18` 隔离验收。实际 HTTP、Artisan、SQLite／MySQL 8.4.13 与 Redis 8.10.3 覆盖并发追加、队列失败回滚、Worker 自身追加、完成后再次追加、旧待发布记录迁移保留、同一批次多次 Redis 追加故障恢复、Redis 队列长度和完成事件。
- 三语追加示例从正文抽取，在真实 HTTP 和独立 Worker 路径执行。VitePress 构建、12 项通用浏览器检查和 6 项追加小节检查通过。
- Rust 1.94.0 格式、全目标严格 Clippy 与锁定离线串行工作区 572 项通过，6 项辅助进程夹具忽略。
