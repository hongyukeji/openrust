# 06-04 任务批次数据库账本

- 核对固定 Laravel [Job Batching](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-batching)、[Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php)、[DatabaseBatchRepository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/DatabaseBatchRepository.php) 与固定 [应用迁移](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/database/migrations/0001_01_01_000002_create_jobs_table.php)。固定 Goravel 的 [Queue 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go) 参考编译型任务边界。旧版 `v0.x`（`6d1a8192`）的 `database/batch.rs` 属主键分批查询，队列 Worker 用例可作为后续运行验收路径，未找到任务批次仓库可复用。
- 在已发布的迁移之后增量建立 `job_batches`，保留现有安装历史。新增 `openrust::bus::{Batch, DatabaseBatchRepository}`。支持创建、查找、增量、成功扣减、失败计数及失败 ID 集、取消、完成与删除。失败保留待处理计数，重试成功移除失败 ID 并保留累计失败次数。批次 JSON 使用 Laravel 字段名、处理数和进度。
- Rust 通过 JSON 保存选项，内部 UTC 时间戳存为有符号 64 位 Unix 秒。计数更新复用数据库事务、MySQL 行锁与并发冲突重试。查找使用写连接。账本方法处理 SQL 状态；派发与 Worker 的幂等终态记账、批次回调和清理命令属于后续运行接线。
- 验收以 `d8cfc51c` 为隔离基线。实际 Artisan 在 SQLite 与 MySQL 8.4.13 验证空库、既有迁移增量升级、单步回滚、重跑、预览与重置。两库验证八个并发完成操作、失败 ID 去重、重试成功、取消、删除与 JSON 进度。
- 三语进度示例均抽取、编译，并以同一迁移测试在 SQLite 和 MySQL 实际执行。VitePress 构建、12 项通用浏览器检查和 6 项批次小节检查通过。
- Rust 1.94.0 工作区 570 项锁定离线串行测试通过，5 项既有辅助夹具忽略。实测 MySQL 和 Redis 连接，格式与全目标严格 Clippy 通过。
