# Webhook 双节点调度与授权故障验收

使用[同主机双节点](topology.md)的独立应用目录、配置缓存、路由缓存及轮询入口，沿用现有数据库、Scheduler 与原生 Worker。固定 Laravel 的单服务器调度负责同一分钟选举，已登记投递的恢复由 Webhook 的持久领取期限控制。队列保留与接收租约仍为独立机制。旧版 `6d1a81924` 无可复用的 Webhook 故障用例。 [固定 Laravel 调度](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#running-tasks-on-one-server) · [固定 Laravel 队列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-expirations-and-timeouts)

- 由节点 0 的真实 `schedule:run` 在持久领取、队列推送前退出。节点 1 同一分钟未获选举，显式回收预览也无法提前领取。等待原有冷却窗口自然结束后，节点 1 调度仅入队一次，独立 Worker 向 HTTP 收件端发送，原正文和 HMAC 保持一致，只有一次投递尝试。
- 在节点 0 的 Worker 进入应用授权阶段后，节点 1 替换端点及凭据。旧投递在发送前被取消，收件端未收到请求。新投递由另一节点发送，签名使用新凭据。将同修订的另一条投递交给缺少新凭据的 Worker，得到 `rejected` 尝试，未触发 HTTP。
- 使用重复的原生数据库队列任务，在旧 Worker 暂停期间将领域租约置为过期。节点 1 完成第二代，旧 Worker 恢复并返回错误时无法覆盖完成状态、诊断或尝试历史。最后的重复队列任务被确认，不再调用接收处理函数。自然到期后的重新领取由前批[拓扑验收](topology.md)和本批基础回归覆盖。

PostgreSQL 18.7 和 MySQL 8.4.13 分别搭配 database 与 Redis 8.10.3，通过四组故障用例及既有拓扑回归。SQLite 单机基础回归通过。验收应用以 `3ade8173d` 冻结源码及本批夹具构建，应用及两个 Webhook 包的格式检查、严格 Clippy 和 Python 语法检查通过。全工作区格式检查仍有[三处既有差异](backend-interruption.md)。使用 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_OWNERSHIP=1` 启用本批路径。

已补[同主机两版本滚动升级](rolling-upgrade.md)。真实跨主机、时差及生产升级仍在 WH-V27。
