# `openrust::queue` 当前事实

## 已实现

- `queue` feature 暴露数据库队列、`Job` / `JobContext`、`QueueDriver` 扩展契约、`QueuePlugin` 与显式 `QueueMigration`。应用先注册 `DatabasePlugin`，再注册队列插件，并在应用 migrator 中注册队列迁移。
- PostgreSQL 与 MySQL 使用数据库事务和 `FOR UPDATE SKIP LOCKED` 并发领取。Job 名称须显式注册；队列名、Job 名、JSON 参数、延迟、最大尝试次数和进度均有边界。
- `dispatch_in_transaction` 与业务事务共享提交/回滚。普通派发独立提交。失败任务保留参数、错误码、次数和重试关联；手动 retry 幂等地产生同一替代 job ID。
- `dispatch_unique` 和 `dispatch_unique_in_transaction` 按 `(queue, unique_key)` 数据库唯一键防止待执行/执行中的重叠派发；重复调用得到同一个活动 job ID。成功确认或迁入 `failed_jobs` 后活动 job 行删除，键随之释放。它不提供完成后的永久请求幂等记录。
- `dispatch_chain` 顺序运行显式命名的子任务；`dispatch_batch` 按 1..=64 并发运行最多 128 项。组作为单个持久 parent job 储存，支持 `Stop` 与 `Continue` 失败策略、事务内派发及 completed/failed/total 进度。parent retry 会从头执行整个组，已成功子任务可能重跑；handler 仍须幂等。停止策略会取消尚在运行的异步 future，外部已提交副作用结果不确定。
- 自定义 `QueueDriver` 需要显式实现 task group dispatch；默认实现返回 `queue_driver_group_unsupported`。
- Worker 支持有界进程内并发、租约心跳、ownership token 状态保护、指数退避、任务超时和租约过期后的崩溃恢复；SIGTERM/Ctrl-C 后停止领取并通知 handler 合作取消，等待配置期限，合作成功按 token 确认，超时按 token 释放为可重试任务；数据库连接池不可用时错误向 supervisor 传播、lease 保留等待到期；supervisor drain、join 全部 worker 并传播错误。PostgreSQL 18/MySQL 8.4 的真实 SIGTERM 验证覆盖 pending 任务不领取、合作成功确认、不合作任务超时/释放租约、连接池故障及旧 worker token 不能影响被接管任务；强杀 lease 恢复测试仍覆盖进程崩溃路径。
- `JobContext::set_progress` 只写有界观测数据，不能作为业务事务或副作用确认。至少一次执行要求应用 handler 幂等；框架不保证数据库提交与邮件、短信或 HTTP 副作用 exactly-once。
- `queue` 启用时内置 `openrust.notification.email.v1` 与 `openrust.notification.sms.v1` 两种 handler。`NotificationsPlugin` 必须在 `QueuePlugin` 之后注册；应用还需按实际渠道注册 `MailerPlugin` / `SmsPlugin`。标准 `queue work` 通过 `JobContext::service<T>` 从该应用的 `AppState` 取发送服务；直接 `Queue::run_next` 不带应用服务上下文。
- `openrust::notifications::Notifier` 把邮件和 SMS 拆成独立 job，固定保存入队时已准备的内容，不在重试时重新读取 locale/模板。`dispatch_in_transaction` 可将多个渠道与业务更新原子提交；普通多渠道 `dispatch` 独立插入各任务，后续渠道错误不会撤销先前已提交任务。活动唯一键为每个渠道派生隔离摘要，活动 job 完成或归档后释放。
- 通知队列 payload 持久化收件地址、号码和消息内容；不记录到框架日志。调用方须保护队列数据库与备份，并避免放入密码、token、验证码等凭据。SMTP/Twilio 受理后、队列确认前故障可导致重复投递；仍是至少一次，不承诺外部 exactly-once。真实 Twilio 投递仍须授权凭据与目标。
- 使用者可实现公开 `QueueDriver` 并以 `Queue::with_driver` 创建 facade；当前 `JobContext` 无公开构造器、进度直接依赖 `Db`、registry 内部可见且 `QueuePlugin` 装配数据库 driver。外部实现尚不能只通过公开 API 完整执行同一 typed Job；可选 Redis/sync driver 前须补执行边界，不能把派发接口当完整可插拔证明。

## M4 尚未完成

- 生产负载下 scheduler 的跨节点/资源边界验收，最终归入 M4 集群门禁；其本地多进程正确性证据见 [core/schedule.md](schedule.md)。
- 类型化事件与显式 queued listener 见 [events 事实](events.md)；持久化任务继续遵循至少一次执行契约。
- 其余专项：多实例生产负载与完整集群边界仍需按 [todo.md](../todo.md) 和 [集群计划](../plans/v1-cluster-deployment.md) 验收；单项 SIGTERM 通过不等于完整 M4/集群验收。
- 生产多实例负载和完整集群验收；单项 SIGTERM/强杀恢复及 scheduler 多进程测试不替代完整部署拓扑。执行任务见 [todo.md](../todo.md)。

当前合同以 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md) 为准。该文件是 M4 总契约，不表示上列未完成项已经实现。
