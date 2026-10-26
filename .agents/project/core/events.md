# `openrust::events` 当前事实

## 当前实现

- `EventsPlugin` 在 `AppBuilder` 注册 `Events` 服务，并按 listener 注册顺序保存类型化 handler；一个 handler 只接收其具体 Rust 事件类型。
- `Events::dispatch` 顺序 await listeners；首个错误 fail-fast 返回，后续 handler 不运行。分发无注册 listener 时成功 no-op。
- `EventContext::transaction()` 在 `dispatch_in_transaction` 时返回原始 transaction 引用。若 listener/queued job 返回错误，调用方需把错误传播出 `Db::transaction` 以回滚业务变化。事务内直接 listener 只能执行可回滚的校验/数据库逻辑，不得发送邮件、短信或其他不可回滚远程副作用；这类工作使用 queued listener。
- 标准 `DatabasePlugin` 与 `EventsPlugin` 同时安装时，`Db::transaction` 自动派发 `TransactionBeforeCommit` 并在提交后派发 `TransactionCommitted`。提交前 listener 按注册顺序执行并可中止/回滚；提交后只执行 direct listeners，错误记录稳定错误码且不改变已提交结果，queued listener 不会在提交后触发。可靠通知需在事务内排入队列，不能依赖进程内提交后通知。
- 启用 `queue` feature 后 `EventsPlugin::listen_queued<E, J>` 将应用事件映射到已注册 Job；`QueuePlugin` 必须先注册。事务分发走 `Queue::dispatch_in_transaction`，普通分发直接持久化 job。Queue handler 仍需显式注册并通过 worker 命令启动。
- 普通进程内 handler 没有持久化、跨节点广播或 exactly-once 保证。`TransactionCommitted` direct handler 是提交成功后的尽力通知，进程退出时可能丢失。直接数据库连接操作绕过 `Db::transaction`，不触发生命周期事件。完整边界见 [ADR-041](../decisions/adr-041-transaction-and-auth-flow-events.md)。
- Event/Job 数据结构由应用定义。不得把密码、JWT/refresh/PAT、验证码、raw request/response 或不必要的个人信息写入队列 payload；错误及 retry 按队列的 at-least-once 边界处理。

## 验证

- `cargo test -p openrust --all-features events::tests`：listener 类型隔离、顺序及首错短路（各 1 项，queued mapper fake test 另测）。
- `cargo test -p openrust --features queue --lib events::queued_tests`：fake QueueDriver 检查成功 payload 映射、mapper error 传播/短路、queue dispatch error 传播/短路，3 项已通过。
- `cargo test -p openrust-server --features queue,db-postgres --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact`：真实 PostgreSQL 18.6 隔离库通过；事务回滚不留 event job、提交后 worker 消费、普通 dispatch 消费及 transaction context 断言均通过。
- 同测试使用 `--features queue,db-mysql`：仓库 Compose 临时 MySQL 8.4 隔离服务上通过；测试后仅清理本次 `openrust-ci` 容器/网络/临时卷，预置 MySQL 容器未修改。
- 真实验收发现 Events 句柄保有 Queue 数据库池；测试关闭隔离库前显式释放 Events、Queue、Db 与 App，随后 `TestDatabase::close` 成功完成清理。
- 临时下游 consumer crate 在 `/tmp/openrust-events-consumer` 使用 `queue,db-postgres`，离线 `cargo check` 通过；验证了外部 crate 可见的 listener、queued Job、插件注册和 transaction dispatch 签名，不代表生成项目或数据库运行验收。

AuthFlows 注册/login validation、success 与 generic failure typed events 按 [ADR-041](../decisions/adr-041-transaction-and-auth-flow-events.md) 提供；上传生命周期扩展仍待后续功能簇实现，不建立通用 Hooks 系统。
