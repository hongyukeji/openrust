# `openrust::queue` 当前事实

## 已实现

- `queue` feature 暴露数据库队列、`Job` / `JobContext`、`QueueDriver` 扩展契约、`QueuePlugin` 与显式 `QueueMigration`。应用先注册 `DatabasePlugin`，再注册队列插件，并在应用 migrator 中注册队列迁移。
- PostgreSQL 与 MySQL 使用数据库事务和 `FOR UPDATE SKIP LOCKED` 并发领取。Job 名称须显式注册；队列名、Job 名、JSON 参数、延迟、最大尝试次数和进度均有边界。
- `dispatch_in_transaction` 与业务事务共享提交/回滚。普通派发独立提交。失败任务保留参数、错误码、次数和重试关联；手动 retry 幂等地产生同一替代 job ID。
- `dispatch_unique` 和 `dispatch_unique_in_transaction` 按 `(queue, unique_key)` 数据库唯一键防止待执行/执行中的重叠派发；重复调用得到同一个活动 job ID。成功确认或迁入 `failed_jobs` 后活动 job 行删除，键随之释放。它不提供完成后的永久请求幂等记录。
- `dispatch_chain` 顺序运行显式命名的子任务；`dispatch_batch` 按 1..=64 并发运行最多 128 项。组作为单个持久 parent job 储存，支持 `Stop` 与 `Continue` 失败策略、事务内派发及 completed/failed/total 进度。parent retry 会从头执行整个组，已成功子任务可能重跑；handler 仍须幂等。停止策略会取消尚在运行的异步 future，外部已提交副作用结果不确定。
- 自定义 `QueueDriver` 需要显式实现 task group dispatch；默认实现返回 `queue_driver_group_unsupported`。
- 外部 driver 可用公开 `JobRegistry::register_job` 注册 typed Job、以 `JobRegistry::execute` 反序列化并执行；`JobContext::new` 接受统一 `Cancellation`，`metadata` / `remaining_budget` 读取每次尝试的作用域信息，`with_progress_reporter` 显式绑定进度存储，`with_app_state` 可提供应用服务上下文。未配置 reporter 时 `set_progress` 返回 `queue_progress_unsupported`；上下文不会公开数据库队列的租约 owner/token。完整内存 driver 接入示例位于 [`examples/queue-custom-driver/`](../../../examples/queue-custom-driver/README.md)。
- `Queue` 从当前 `ExecutionContext` 传递 `ExecutionMetadata`，仅允许 request ID、locale 和经校验的 tenant ID；tenant 由 `TenantContext` 在 dispatch 处捕获，并在 worker attempt 内恢复。身份、权限快照及凭据不进入 envelope，handler 必须重新校验权限。损坏的框架 envelope 返回 `queue_execution_metadata_invalid`，不会作为旧应用 payload 继续执行。数据库 driver 使用版本化 payload envelope；legacy 原始任务仍可执行，retry 和任务组保留关联元数据。自定义 driver 必须实现新增 metadata-aware 派发方法；旧实现遇到非空 metadata 默认返回 `queue_driver_metadata_unsupported`。`__openrust_execution` 是保留 payload 字段。
- 每个 worker attempt 均建立新的 deadline/cancellation 作用域；任务组子项继承该次 attempt，重试会创建新作用域。request ID 只做日志/span 关联，不是 W3C trace parent；task-local 不跨 `tokio::spawn` 自动传播。
- `openrust generate job <name>` 为数据库应用生成 serde payload 类型、`Job` handler、`JobsPlugin` 注册入口及 `QueueMigration` 登记，并同时启用 app/migration 的 `queue` feature。未实现 handler 返回 `job_not_implemented`，不会被误认为成功；API、worker、scheduler 通过同一应用分别运行，`serve` 不会自动拉起后台进程。组合示例位于 [`examples/queue/`](../../../examples/queue/README.md)。
- Worker 支持有界进程内并发、租约心跳、ownership token 状态保护、指数退避、任务超时和租约过期后的崩溃恢复；SIGTERM/Ctrl-C 后停止领取并通知 handler 合作取消，等待配置期限，合作成功按 token 确认，超时按 token 释放为可重试任务；数据库连接池不可用时错误向 supervisor 传播、lease 保留等待到期；supervisor drain、join 全部 worker 并传播错误。PostgreSQL 18/MySQL 8.4 的真实 SIGTERM 验证覆盖 pending 任务不领取、合作成功确认、不合作任务超时/释放租约、连接池故障及旧 worker token 不能影响被接管任务；强杀 lease 恢复测试仍覆盖进程崩溃路径。
- `JobContext::set_progress` 只写有界观测数据，不能作为业务事务或副作用确认。至少一次执行要求应用 handler 幂等；框架不保证数据库提交与邮件、短信或 HTTP 副作用 exactly-once。
- `JobRegistry::execute` 与数据库 worker 都为 typed Job 建立独立的 `ExecutionContext` task-local，并在有 `I18n` 应用服务时使用 metadata locale 或应用默认 locale。两种入口都执行 context deadline 与合作取消；外部 driver 仍负责重试和 worker 租约策略。
- `JobRegistry::execute` 是显式的进程内、await 后返回的 typed Job 执行入口，可用于同步业务调用或无数据库/Redis 的测试。它不会派发或持久化任务，不提供事务派发、自动重试或任务组；调用方提供一次尝试的 `JobContext`。取消须由 handler 合作处理，进度须显式配置 reporter；需要持久化恢复时使用 `Queue`/driver。
- `queue` 启用时内置 `openrust.notification.email.v1` 与 `openrust.notification.sms.v1` 两种 handler。`NotificationsPlugin` 必须在 `QueuePlugin` 之后注册；应用还需按实际渠道注册 `MailerPlugin` / `SmsPlugin`。标准 `queue work` 通过 `JobContext::service<T>` 从该应用的 `AppState` 取发送服务；直接 `Queue::run_next` 不带应用服务上下文。
- `openrust::notifications::Notifier` 把邮件和 SMS 拆成独立 job，固定保存入队时已准备的内容，不在重试时重新读取 locale/模板。`dispatch_in_transaction` 可将多个渠道与业务更新原子提交；普通多渠道 `dispatch` 独立插入各任务，后续渠道错误不会撤销先前已提交任务。活动唯一键为每个渠道派生隔离摘要，活动 job 完成或归档后释放。
- 通知队列 payload 持久化收件地址、号码和消息内容；不记录到框架日志。调用方须保护队列数据库与备份，并避免放入密码、token、验证码等凭据。SMTP/Twilio 受理后、队列确认前故障可导致重复投递；仍是至少一次，不承诺外部 exactly-once。真实 Twilio 投递仍须授权凭据与目标。
- 外部 `QueueDriver` 可用 `Queue::with_driver` 创建 facade，并由驱动自行持有 `JobRegistry` 执行 typed Job。事务派发、任务组、失败保留/重试等不支持能力必须明确失败；本地内存示例不是持久化或跨节点实现。
- 可选 `queue-redis` feature 提供 `RedisQueueDriver`，也可由 `QueuePlugin` 配置 `queue.driver=redis` 选择；`queue` 默认仍使用数据库驱动。Redis-only 应用无需 `DatabasePlugin`，但 Redis URL 和独立 namespace 必填；无法连接时失败，不回退到本地或数据库。Redis driver 使用 Lua 原子派发/领取/租约/确认/失败迁移，Redis 服务端时钟与 token fencing；多个连接共享领取，租约过期后自动重试，达到最大次数后进入共享 failed 状态，手动 retry 幂等返回同一替代 job ID。handler 仍是至少一次语义，须自行幂等；Redis 持久性依赖服务端配置，不承诺 Redis 与业务数据库事务原子性。
- Redis Queue 不提供数据库事务派发、任务组或数据库调度。注册 schedule 时启动失败，避免误示事务和调度原子性；调度仍使用数据库 QueuePlugin 默认路径。Redis 普通单端点/代理地址须让本实例脚本涉及的 key 共置同一 Redis Cluster slot，但自动集群分片与跨槽迁移不在支持范围内。

## M4 尚未完成

- 生产负载下 scheduler 的跨节点/资源边界验收，最终归入 M4 集群门禁；其本地多进程正确性证据见 [core/schedule.md](schedule.md)。
- 类型化事件与显式 queued listener 见 [events 事实](events.md)；持久化任务继续遵循至少一次执行契约。
- 其余专项：多实例生产负载与完整集群边界仍需按 [todo.md](../todo.md) 和 [集群计划](../plans/v1-cluster-deployment.md) 验收；单项 SIGTERM 通过不等于完整 M4/集群验收。
- 生产多实例负载和完整集群验收；单项 SIGTERM/强杀恢复及 scheduler 多进程测试不替代完整部署拓扑。执行任务见 [todo.md](../todo.md)。

当前合同以 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md) 为准。该文件是 M4 总契约，不表示上列未完成项已经实现。
