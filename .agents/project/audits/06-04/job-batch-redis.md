# 06-04 Redis 批次派发与恢复

- 对照固定 Laravel [Job Batching](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-batching)、[Batch::add](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) 和 [RedisQueue::bulk](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/RedisQueue.php)。参考固定 Goravel [Queue 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)。旧版 `v0.x` 的 `queue/redis.rs` 已用 Lua 原子处理队列状态，未实现 SQL 批次账本与 Redis 发布联动。
- `Bus::batch(...).onConnection("redis")` 使用同一 Redis 连接和队列发布整批任务。SQL 原子保存账本与 `job_batches_outbox` 待发布记录。Redis Lua 校验队列键类型后写入任务、通知和去重标记。SQL 按发布记录锁定发布，确认后才删除待发布记录。标记在 SQL 确认前无 TTL，确认后尝试设置 24 小时过期。Redis 持久化或淘汰造成的数据丢失由 Redis 运维策略决定。
- 跨存储无法共享数据库事务。Redis 发布失败时返回已受理批次并记录待恢复状态，`queue:publish-batches` 可重复执行。发布事件在 SQL 确认后派发，仍遵循已有进程内事件边界。数据库队列保持共享连接原子写入。自定义批次表需要 `_jobs` 与 `_outbox` 配套表。
- 基于 `e3726c81` 的隔离验收。真实 HTTP 和 Artisan 在 SQLite／MySQL 8.4.13 与 Redis 8 上验证即时发布、Redis 键类型错误保留 SQL 意图、Redis 写入后 SQL 确认失败、两个发布进程并发恢复、Redis 队列长度保持为两条、独立 Worker 处理完成与发布事件。新增迁移在两库验证增量安装和回滚。
- 三语恢复命令从正文抽取，经实际 Artisan 和 Redis 8.10.3 分别验证 SQLite 与 MySQL。VitePress 构建、12 项通用浏览器检查和 6 项 Redis 批次小节检查通过。
- Rust 1.94.0 格式、全目标严格 Clippy 与工作区锁定离线串行复验 572 项通过，6 项辅助进程夹具忽略。首轮触发既有 SQLite 认证测试对随机密文包含 `001` 的偶发断言，完整复验通过。
