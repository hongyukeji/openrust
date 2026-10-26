# M4 类型化事件与队列 listener Spec

- 目标：按 ADR-037 提供应用级类型化事件注册、顺序通知和显式耐久 listener，不新增通用 Hooks 容器。
- 注册：`EventsPlugin` 在 AppBuilder 阶段按声明顺序收集 listener；一个事件类型可有多个同步异步 handler。
- 分发：`Events::dispatch` 顺序 await；第一个错误直接返回并停止后续 handler；不捕获、不重试、不吞错。
- 事务：`dispatch_in_transaction` 把调用方 transaction 放入只读 `EventContext`；同步 listener 出错由调用方传播以回滚，queued listener 使用相同 transaction 派发任务。
- 队列：启用 `queue` feature 后，`listen_queued<E, J>` 把事件映射为已注册 `Job`；普通分发独立派发，事务分发原子派发。事件与 Job 均不自动推断、注册或启动 worker。
- 可靠性：进程内 listener 不持久、不跨节点、不保证崩溃后重放；可靠副作用必须在业务 transaction 中使用 queued listener 或显式事务内派发。外部副作用仍按 Queue 至少一次语义幂等处理。
- 失败：事件映射、序列化、queue/store 和同步 listener 错误均 fail-fast；事务内错误由外层事务决定 rollback；提交后再 dispatch 的错误不代表已提交业务回滚。
- 安全：事件 payload 由应用类型定义，文档禁止携带密码、token、验证码、raw request 或不必要 PII；队列 payload 沿用 1 MiB 限额。
- 验收：顺序、短路、错误传播、transaction context、queued Job 映射与生成式外部应用用法；随后在 PG18/MySQL8.4 真实隔离库验证事务回滚不留任务、提交后任务可领取及 handler 执行。
- 排除：文件存储前后流程、AuthFlows 前置策略/成功失败事件、多渠道邮件短信通知与请求幂等由后续功能簇实现，不以本 EventBus 替代其 contract。
