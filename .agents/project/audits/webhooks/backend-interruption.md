# Webhook 共享后端断连与恢复

延续[双节点拓扑](topology.md)，在两个应用节点与各隔离共享服务之间接入本机 TCP 转发。关断时终止已建立连接并拒绝新连接，恢复时保持原端口、应用进程及后端数据。使用隔离数据库和 Redis 自身的查询观察权威记录。本项目既有 Redis 断响应夹具验证单命令回执丢失，本批验收复用隔离后端和真实 Artisan 路径，专门覆盖两个节点共同失去后端连接的窗口。旧版 `6d1a81924` 无同类 Webhook 验收。

## 验收范围

- 主写库失联时，两节点的 HTTP 接收和新投递登记返回失败，接收及投递记录数保持不变。已有投递仍为待处理，尝试次数为零，对端未收到请求。恢复连接后，两节点原 HTTP 进程重新接收，独立 Worker 读取正文，已有投递完成一次真实 HTTP 发送。
- Redis 失联时，已配置持久派发恢复的接收入口仍可在主写库提交正文和派发意图后确认，两节点的重复提交只产生一条意图。Scheduler 无法完成共享缓存选举，接收与出站回收命令返回失败，数据库队列未接管这些任务。出站投递保持零尝试。
- Redis 恢复连接后，等待真实领取冷却窗口结束。两节点同时扫描，各恢复一个队列项。独立 Worker 核对收件 ID 及原文，HTTP 收件端核对出站 ID、原文和 HMAC，最终队列为空。沿用既有共享缓存选举、具名队列连接及应用显式选配的回退机制。 [固定 Laravel 调度](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#running-tasks-on-one-server) · [固定 Laravel 队列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md)

`tests/webhooks_topology_acceptance.py` 以 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_OUTAGE=1` 启用本批路径。PostgreSQL 和 MySQL 使用既有隔离包装器，额外设置 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_REDIS=1` 可验证 Redis 隔离断连。

Rust 验收制品从 `c851d1e8a` 的隔离源码重新编译。PostgreSQL 18.7 和 MySQL 8.4.13 分别搭配 database 和 Redis 8.10.3 的四种配置均通过断连及原有拓扑用例，SQLite 单机回归通过。`openrust` 及两个 Webhook 包的格式检查、实际验收应用的严格 Clippy 和 Python 脚本语法检查均通过。该源码快照的全工作区格式检查报告既有差异，位于 `fortify/tests/two_factor_provider.rs`、`framework/src/database/events.rs` 和 `framework/src/queue/events.rs`。

真实跨主机网络分区、主库主备切换、Redis 数据丢失、远端对象存储断连和部署滚动升级仍在 WH-V27。当前夹具保留共享服务进程及已有数据。
