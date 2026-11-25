# Webhook 单机与双节点验收

同一 Rust 验收制品同时启用接收和发送包。双节点使用独立应用目录、配置缓存、路由缓存和 HTTP 进程。测试负载均衡逐请求轮询，HTTP、Artisan Worker 和 Scheduler 都通过真实入口。两个应用节点运行在同一台 macOS ARM64 主机，数据库和 Redis 使用隔离 Docker 容器。附件通过两节点共同可访问的私有目录读取。

| 权威数据库 | 队列 | 调度缓存 | HTTP 节点 | 结果 |
| --- | --- | --- | ---: | --- |
| SQLite | database | database | 1 | 通过 |
| PostgreSQL 18.7 | database | database | 2 | 通过 |
| MySQL 8.4.13 | database | database | 2 | 通过 |
| PostgreSQL 18.7 | Redis 8.10.3 | Redis | 2 | 通过 |
| MySQL 8.4.13 | Redis 8.10.3 | Redis | 2 | 通过 |

- 五个组合均执行 Artisan 迁移及配置、路由缓存，实测缓存前后的 HTTP。双节点中由节点 0 写入私有附件、节点 1 的 Worker 读取；两节点并发收到同一个已验签事件时，保存一个接收记录并只入队一次。
- 接收节点在持久提交后、派发前实际退出。另一节点仍能确认重试请求，两个回收命令竞争后只入队一次，独立 Worker 核对原始正文。另对 database 队列的接收 Worker 注入领取后退出，超过实际租约后另一 Worker 读取原文并完成新代次，尝试历史保留过期及完成状态。
- 两个 `schedule:run` 进程使用 `on_one_server()` 竞争同一 UTC 分钟的共享缓存选举，首次恢复只执行一次。共享队列消费后，每个投递只有一次已记录尝试，HTTP 收件端验证正文和 HMAC。并发发送跨节点共用作用域额度，第三笔在消耗尝试前等待，其它作用域仍可独立发送。
- Redis 组合以 Redis 承载持久派发和出站队列，并以 Redis 缓存选举。需要与接收记录同库事务的附件和事件仍走 database 队列。固定 Laravel 的单服务器调度和队列连接继续由框架承担，领域状态、领取和限额保存在共享主写库。旧版 `6d1a81924` 无可复用的同类验收。 [固定 Laravel 调度](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md) · [固定 Laravel 队列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md)

`tests/webhooks_topology_acceptance.py` 接收实际编译的 `webhooks_app` 路径。SQLite 用 `OPENRUST_TEST_SMS_SQLITE=1`，PostgreSQL 和 MySQL 使用各自隔离验收包装器。额外设 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_REDIS=1` 验证 Redis。验收制品通过 `cargo fmt --all -- --check` 及两包 feature 下的 `cargo clippy --test webhooks_app -- -D warnings`。数据库、Redis 和临时应用目录已清理。

该证据覆盖 WH-V25、WH-V26 和 WH-V27 的上述路径。WH-V25 仍须补齐拓扑级同步派发及受影响缓存组合；WH-V26 已补[同主机双节点共享 S3 附件](shared-object-storage.md)，真实跨主机、托管对象存储、生产入口和目标操作系统仍待验；WH-V27 已补[共享主写库和 Redis 断连与恢复](backend-interruption.md)。Scheduler 终止、旧 Worker 回归、端点撤销、缺失资源或凭据修订、时差和滚动升级还须在集群拓扑中复验。完整 Webhook 门禁保持开放。
