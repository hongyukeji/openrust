# `openrust::queue` 当前事实

## 已实现

- `queue` feature 暴露数据库队列、`Job` / `JobContext`、`QueueDriver` 扩展契约、`QueuePlugin` 与显式 `QueueMigration`。应用先注册 `DatabasePlugin`，再注册队列插件，并在应用 migrator 中注册队列迁移。
- PostgreSQL 与 MySQL 使用数据库事务和 `FOR UPDATE SKIP LOCKED` 并发领取。Job 名称须显式注册；队列名、Job 名、JSON 参数、延迟、最大尝试次数和进度均有边界。
- `dispatch_in_transaction` 与业务事务共享提交/回滚。普通派发独立提交。失败任务保留参数、错误码、次数和重试关联；手动 retry 幂等地产生同一替代 job ID。
- Worker 支持有界进程内并发、租约心跳、ownership token fencing、指数退避、任务超时、合作式取消、有限停机 drain/join、租约过期后的崩溃恢复。
- `JobContext::set_progress` 只写有界观测数据，不能作为业务事务或副作用确认。至少一次执行要求应用 handler 幂等；框架不保证数据库提交与邮件、短信或 HTTP 副作用 exactly-once。
- 使用者可实现公开 `QueueDriver` 并以 `Queue::with_driver` 创建 facade；自定义 driver 的 plugin/命令注册由应用负责。

## M4 尚未完成

- 任务唯一键、批次 API/结果、任务链与链失败策略。
- 生产负载下 scheduler 的跨节点/资源边界验收，最终归入 M4 集群门禁；其本地多进程正确性证据见 [core/schedule.md](schedule.md)。
- 有序进程内事件 listeners 与显式 queued listener。
- 旧 worker token 接管竞态专项断言、worker/scheduler 命令停机测试和完整多实例集群验收。

当前合同以 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md) 为准。该文件是 M4 总契约，不表示上列未完成项已经实现。
