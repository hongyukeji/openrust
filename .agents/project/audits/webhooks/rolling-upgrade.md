# Webhook 滚动升级验收

选用 `f4461a881` 和 `2a6a06277` 两个实际编译的 Rust 制品。旧版已有独立投递 ID 和键映射，新版新增幂等到期相关列及索引。两制品各自生成配置和路由缓存，共用数据库、队列、凭据及范围。新增清理命令不在混合版本期间启用。旧版工作树 `6d1a81924` 无可复用的 Webhook 滚动升级夹具。 [固定 Laravel 迁移](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#isolating-migration-execution) · [固定 Laravel Worker 部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#queue-workers-and-deployment)

验收脚本 `tests/webhooks_rolling_acceptance.py` 接收当前 `webhooks_app` 路径，通过 `OPENRUST_TEST_WEBHOOK_PREVIOUS_BINARY` 指定旧制品。使用隔离数据库包装器，增加 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_REDIS=1` 选择共享 Redis。以文件摘要断言两个制品各自独立。

## 验收结果

- 旧版两节点运行时，由新版 Artisan 执行一次新增迁移。旧 HTTP 进程在迁移后继续服务。逐个移出负载均衡、替换制品并重新生成缓存，期间由另一个实例继续处理请求。
- 在混合版本阶段，两版都可登记及读取同键同快照，返回同一投递 ID，同键更改正文返回 409。旧 HTTP 登记的接收记录由新版 Worker 完成，新版登记的由旧 Worker 完成。数据库队列的接收完成标记及原文一致。Redis 组合另验证两版的持久接收路由由对方 Worker 消费，出站任务同样双向交接。
- 旧版首次收到 HTTP 503 后保留投递 ID、已用尝试次数、退避时间和截止时间。切换两个节点后，等待原退避时间到期，新版 Artisan 恢复原任务，实际 HTTP 响应 204。历史依次记录 503 和 204，签名和正文保持一致，两类队列最终为空。
- PostgreSQL 18.7、MySQL 8.4.13 各通过 database 与 Redis 8.10.3 双节点组合。SQLite 完成同主机迁移兼容性验证。该结果限于选定的增量迁移。较早的 ID 契约切换、已启用清理后的降级、繁忙生产数据库迁移和真实跨主机仍按各自发布门禁验收。

新制品的应用及两个 Webhook 包通过定向格式检查和验收目标严格 Clippy，Python 语法检查通过。全工作区格式检查仍报告[三处既有差异](backend-interruption.md)。共享轮询夹具的 SQLite 和 PostgreSQL 基础拓扑回归通过。
