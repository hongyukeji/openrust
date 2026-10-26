# `openrust::events` 当前事实

## 当前实现

- `EventsPlugin` 在 `AppBuilder` 注册 `Events` 服务，并按 listener 注册顺序保存类型化 handler；一个 handler 只接收其具体 Rust 事件类型。
- `Events::dispatch` 顺序 await listeners；首个错误 fail-fast 返回，后续 handler 不运行。分发无注册 listener 时成功 no-op。
- `EventContext::transaction()` 在 `dispatch_in_transaction` 时返回原始 transaction 引用。若 listener/queued job 返回错误，调用方需把错误传播出 `Db::transaction` 以回滚业务变化。事务内直接 listener 只能执行可回滚的校验/数据库逻辑，不得发送邮件、短信或其他不可回滚远程副作用；这类工作使用 queued listener。
- 启用 `queue` feature 后 `EventsPlugin::listen_queued<E, J>` 将应用事件映射到已注册 Job；`QueuePlugin` 必须先注册。事务分发走 `Queue::dispatch_in_transaction`，普通分发直接持久化 job。Queue handler 仍需显式注册并通过 worker 命令启动。
- 进程内 handler 没有持久化、跨节点广播、提交后自动回调或 exactly-once 保证。可靠业务副作用使用事务内 queued listener；如果在事务提交后调用普通 `dispatch`，失败只表示通知未成功，不会回滚已提交业务。
- Event/Job 数据结构由应用定义。不得把密码、JWT/refresh/PAT、验证码、raw request/response 或不必要的个人信息写入队列 payload；错误及 retry 按队列的 at-least-once 边界处理。

## 验证

- `cargo test -p openrust --all-features events::tests`：listener 类型隔离、顺序及首错短路（各 1 项，queued mapper fake test 另测）。
- `cargo test -p openrust --features queue --lib events::queued_tests`：fake QueueDriver 检查成功 payload 映射、mapper error 传播/短路、queue dispatch error 传播/短路，3 项已通过。
- `cargo test -p openrust-server --features queue,db-postgres --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact`：真实 PostgreSQL 18.6 隔离库通过；事务回滚不留 event job、提交后 worker 消费、普通 dispatch 消费及 transaction context 断言均通过。
- 同测试使用 `--features queue,db-mysql`：仓库 Compose 临时 MySQL 8.4 隔离服务上通过；测试后仅清理本次 `openrust-ci` 容器/网络/临时卷，预置 MySQL 容器未修改。
- 真实验收发现 Events 句柄保有 Queue 数据库池；测试关闭隔离库前显式释放 Events、Queue、Db 与 App，随后 `TestDatabase::close` 成功完成清理。
- 临时下游 consumer crate 在 `/tmp/openrust-events-consumer` 使用 `queue,db-postgres`，离线 `cargo check` 通过；验证了外部 crate 可见的 listener、queued Job、插件注册和 transaction dispatch 签名，不代表生成项目或数据库运行验收。

AuthFlows 的注册/登录校验与业务结果事件、上传生命周期扩展仍按 `todo.md` 后续功能簇实现；当前不建立通用 Hooks 系统。
