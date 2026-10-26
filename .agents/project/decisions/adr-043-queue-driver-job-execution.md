# ADR-043 队列 driver 复用 typed Job 执行器

- 状态：Accepted and implemented
- 日期：2026-10-26

## 背景

`QueueDriver` 可由外部实现，但此前其无法使用私有 registry 执行 `QueuePlugin` 注册的 typed Job。`JobContext` 的进度实现直接绑定数据库 worker，导致外部 driver 不能构造同等任务上下文。

## 决策

- `JobRegistry` 提供公开注册与异步执行 API，外部 driver 可持有它并执行经名称登记的 `Job`。
- 外部 driver 使用 `JobContext::new` 提供任务 ID、尝试次数和 [`Cancellation`](adr-044-cross-protocol-execution-context.md)；非法元数据被拒绝。任务 metadata 和剩余预算由只读 getter 提供。
- 进度 reporter 由 driver 显式提供。未提供时 `set_progress` 返回 `queue_progress_unsupported`；框架数据库 worker 的 reporter 仅在内部持有 Db 与租约 ownership token。
- typed Job 执行保留 serde 反序列化、参数边界及错误传播；未注册任务返回 `unregistered_queue_job`。
- `QueueDriver` 对不支持的事务派发、任务组、重试等能力必须明确失败，不得伪装成功。外部 driver 自行定义持久化、取消、恢复与副作用语义。
- 此扩展不增加官方数据库队列后端，也不改变内置 PostgreSQL/MySQL driver 的至少一次和租约语义。

## 影响

外部 driver 可复用框架 Job 与 `Queue` facade，同时保留后端对进度、事务和恢复边界的责任。示例见 [`examples/queue-custom-driver`](../../../examples/queue-custom-driver/README.md)。
