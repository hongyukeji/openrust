# ADR-037 数据库队列、调度与 worker 生命周期

- 状态：Accepted（M4 contract；worker 合作停机与 join 修正已实现，F1 日历补强及专项/集群验收待完成）
- 日期：2026-10-26
- 依据：[ADR-003](adr-003-database-queue-default.md)、[ADR-023](adr-023-loco-application-experience-evaluation.md)、[v1 能力覆盖](../plans/v1-capability-coverage.md)、[集群计划](../plans/v1-cluster-deployment.md)

## 范围

- 在根 package 提供 `openrust::queue` 和 `openrust::events`；不创建独立 crate、不恢复空模块、不接入 apalis 或外部消息总线。
- 数据库队列是 v1 官方 driver，复用 `openrust::db::Db`、SeaORM 事务和应用 migrator。Redis queue driver 不属于本 ADR 实现范围。
- V1 必须在现有 Scheduler 上交付 cron、IANA 时区、DST 与有界错过触发处理，详细范围见 [补齐计划](../plans/v1-foundation-completion.md)；不替换既有队列或新建调度系统。
- Job 使用稳定、显式注册的类型名与 serde JSON 参数；数据库不反序列化任意 Rust 类型或执行未注册 handler。队列名、类型名和 payload 大小均有边界。
- Job trait 提供类型化任务与 `JobContext`。Context 暴露 attempt、最大尝试次数、任务 ID、合作式取消检查和有界进度更新；进度只作为观测信息，不作业务事务或外部副作用凭证。
- `Queue` 支持普通派发、延迟派发、显式队列/重试策略、任务唯一键、事务内派发、有限并发批次与受支持的任务链。批大小、链长度和并发有配置上限；失败停止/继续语义由 API 显式选择。
- 队列至少一次执行。Handler 可在提交业务事务前后重试；调用方必须使业务处理幂等。数据库提交不能与外部邮件、短信、HTTP 或其他供应商副作用形成原子事务；需使用供应商幂等键或业务 outbox。

## 持久化和领取

- `QueueMigration` 显式创建 resolver-aware `jobs`、`failed_jobs`、批次状态和调度触发记录；应用需在其 migrator 中注册，不在请求或 QueuePlugin 启动时自动迁移。
- `jobs` 保存稳定 job name、JSON payload、queue、attempts/max attempts、available time、first/last reserved time、owner、每次领取的新 ownership token、lease expiry、timeout、progress、唯一键和创建时间。失败迁移到 `failed_jobs` 时保留原始参数、最终错误摘要和尝试数；错误文本限制长度且不得包含凭据。
- 领取顺序为 `available_at, id`。PostgreSQL/MySQL 在事务中 `SELECT ... FOR UPDATE SKIP LOCKED` 后更新为已领取并写入新 token/lease，再提交后运行 handler。SQL 参数绑定；仅框架静态方言片段可拼接。
- 所有 lease 和调度触发时间以共享数据库 UTC 时钟为准。Worker 以心跳延长 lease；完成、失败、续约和取消更新都必须匹配当前 ownership token。旧 worker 恢复后不能确认、删除或释放新 worker 已接管的任务。
- 进程崩溃时不执行本地清理；lease 到期后任务可重新领取并消耗一次尝试。无法判断崩溃前的外部副作用是否已完成，因此重复执行仍属于 at-least-once 契约。
- 可配置固定或指数退避及最大延迟。每次领取增加 attempts；未超过 max attempts 的失败重新排队，耗尽后与移除原 job 在同一事务写入 `failed_jobs`。数据库错误保持 lease 等待过期恢复，不伪装为成功。
- 手动重试只能从 `failed_jobs` 受控重新派发，并保留来源记录或明确其状态变化；不得将失败记录静默删除。
- `dispatch_in_transaction` 接收同一 `Db` 的调用方 `DatabaseTransaction`，在业务 SQL 事务内插入 job；回滚时任务不可见。普通 `dispatch` 使用独立数据库事务。仅在提交成功后 worker 才可领取。

## 调度、锁和事件

- 当前 Scheduler 注册稳定 schedule key 与 UTC interval/daily slot；任务触发槽的唯一约束与 job 插入在同一事务完成。多个 scheduler 竞争同一槽时仅一个事务可派发，事务失败时槽标记回滚。V1 日历计划仍派生共享数据库 UTC 时间槽，身份须覆盖时区、DST 重复时间和计划变更。
- `schedule run --once` 执行一次 due scan；常驻模式按配置 poll。当前 daily 仅 UTC；V1 必须新增成熟 cron 解析与 IANA 时区支持。实施 Spec 明确语法、DST 缺失/重复时间、计划变更、错过触发的跳过/有界补跑及重叠语义，限制每 tick 扫描和派发量；具体建议见补齐计划，不提供自然语言或任意 shell 调度。
- 数据库锁通过独立 lock key、owner token 和数据库过期时间保护；只有当前 token 可释放。锁 TTL 到期允许其他节点接管，无法证明旧持有者已停止，因此保护共享写入时必须用数据库事务/条件更新 fencing，不可只依赖租约宣称无重叠。
- `openrust::events` 默认提供进程内、顺序 listener dispatch；注册顺序确定，listener 错误按 fail-fast 返回且停止后续 listener。调用方可显式把 listener 投递至 Queue。进程内事件不是可靠消息、跨节点广播或事务 outbox。
- 事务提交前处理属于同步业务逻辑；提交后事件可能在进程故障时丢失。要求可靠跨进程投递时调用方必须在业务事务中 `dispatch_in_transaction`，不依赖进程内 after-commit 回调。

## worker / scheduler 安全停机

- Worker 与 Scheduler 是显式运行时命令，由命令调用持有 supervisor、并发任务集合和退出信号；普通 API serve 不暗中启动 worker。该所有权模型不需要给所有 Plugin 新增通用 shutdown hook，也不新建并列 Hooks 系统。
- 收到 Ctrl-C/SIGTERM 后，停止领取/扫描新任务；对运行中 handler 发出合作式取消信号，等待至 `[queue].shutdown_timeout_secs` 截止。成功完成的任务按正常 token 条件确认。
- 超时后取消剩余 handler future，尝试按 ownership token 将 lease 转为可重试状态；若数据库不可达则保留 lease，等待可见性超时。Supervisor 中每个 task 都必须被 join；取消后的 handler 副作用结果不确定，仍按至少一次执行处理。
- 单任务 timeout 取消 handler future 并按重试策略处理；不合作任务不会无限阻止 worker 退出。`spawn_blocking` 或外部系统中已提交的工作不能由 Tokio future 取消，文档必须说明应用自行保持幂等。
- Scheduler 收到停机信号后不开始新 tick，等待当前短事务结束后退出；尚未提交的触发槽和 job 一并回滚。节点强制退出由数据库连接回滚、唯一触发槽和 job lease 恢复。
- API 端点 drain 与 worker/scheduler drain 是不同生命周期。`openrust queue work`、`openrust schedule run` 的进程退出码反映 supervisor 和 join 失败。

## 验收

- 单元与 HTTP/命令测试：注册 handler allowlist、参数边界、调度 slot 计算、退避上限、任务链顺序/短路、批次并发上限、进度与合作式取消、关停停止领取、有限 drain/abort/join。
- PostgreSQL 18 与 MySQL 8.4：双 worker 并发无重复当前领取、事务内派发随提交可见/回滚消失、延迟与退避、失败迁移/人工重试、lease 续约/过期接管、旧 ownership token 不能确认新 lease、调度多实例同槽只插入一次、节点中断后自动恢复。
- 真实异常退出测试须启动多独立 worker/scheduler 共享同一数据库，强制终止持有任务的进程并观测 lease 过期恢复；fake 或互不通信的数据库不能替代。
- Worker 生命周期修正已在 PostgreSQL 18/MySQL 8.4 隔离数据库验证：真实 SIGTERM 下合作 handler 正常确认、不合作 handler 有界超时并以 `queue_shutdown_timeout` 失败、pending job 不再领取；关闭 worker 连接池模拟数据库不可达，命令传播 `internal_error` 且 lease 保留；接管过期 lease 后旧 token 无法确认新 worker 的结果。既有 SIGKILL 崩溃恢复仍单独验证；生产负载和完整多实例集群继续开放。
- 日历补强另验跨日/闰年、DST 缺失与重复、停机补跑上限、计划变更、双节点竞争与崩溃恢复；双库验证 UTC slot 去重和派发事务，不能用已通过的 interval/daily 验收替代 cron/IANA 验收。
- M4 首版不承诺外部副作用 exactly-once、不自动从失败任务重放任意事件、不提供任意 DAG 工作流，也不自动启动 worker。
