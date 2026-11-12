# 06-04 · 任务链连接、队列与延迟

## 固定基线与旧版

- 对照 Laravel [Chain Connection and Queue](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#chain-connection-queue)、[PendingChain](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bus/PendingChain.php)、[Queueable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Queueable.php) 及 [JobChainingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/JobChainingTest.php)。
- 参考固定 Goravel [Queue::Chain](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型参数和 Worker 连接契约。旧版 `v0.x:vendor/openrust/framework/src/queue.rs` 的 `DispatchOptions` 与 `dispatch_chain` 支持派发组及首项延迟，组内任务没有独立连接、队列和延迟。沿用旧版链顺序断言与主线预约约束。

## 实现与差异

任务载荷的 `onConnection`、`onQueue`、`delay` 覆盖该任务，后继仍继承链默认值。旧载荷缺少新字段时沿用现有预约目标。SQLite、MySQL 和 Redis 在同连接中原子确认预约并按新队列及延迟入队。

跨持久连接使用源队列中的转发任务。目标 SQL 在同一事务保存投递凭据并入队，Redis 通过 Lua 写入凭据与后继。源 Worker 在目标确认后才删除转发任务。故障后重启或继续运行源 Worker 即可恢复。独立转发任务会占用一次 Worker 处理，投递凭据保留供重放去重。持久队列转到 sync 的路径尚待实现。

## 验收

- 实际 HTTP 和独立 Artisan 验证首项覆盖、同连接换队列和延迟、后继恢复默认值、SQLite／MySQL 8.4.13 与 Redis 8.10.3 双向交接。分别注入 Redis 键类型故障与数据库入队故障，验证重试及重放后目标只入队一次。验证持久队列转 sync 的明确拒绝。
- Rust 1.94.0 格式检查、严格 Clippy、锁定离线工作区测试。三语示例接入实际 HTTP 和 Artisan 夹具，站点构建及 18 项浏览器检查。

[English](../../../../docs/en/digging-deeper/queues.md#chain-connection-queue) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#chain-connection-queue) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#chain-connection-queue)
