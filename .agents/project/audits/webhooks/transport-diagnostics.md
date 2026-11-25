# 持久化投递尝试的传输诊断

固定 Spatie 的 Job 拿到传输结果后将统计交给结果事件。OpenRust 已在[进程内结果事件](transport-events.md)表达该契约。持久查询复用 Webhook 投递尝试表和 `DeliveryStore` 的范围约束。结果在完成尝试的同一事务内写入实际网络传输微秒数或脱敏失败代码，仍受原领取令牌约束。旧 Worker 后续完成的尝试、运行时模拟响应、撤销或崩溃回收可以没有测量值。旧令牌无法补写已回收尝试的测量结果。 [固定 Spatie CallWebhookJob](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php)

已有尝试列表和 JSON 形状沿用原契约。新增 `DeliveryStore::inspect_attempt_transport` 与 `webhook-server:inspect --transport=<尝试编号>` 读取一条授权范围内的诊断；范围内存在记录但没有观测时返回两个空值，尝试不存在或属于其他范围时返回 `null`。诊断列只有数字，Worker 不保存响应正文或底层错误。查询使用原有投递 ID 和尝试编号。新增两列默认空值，先迁移再启动新版 Worker。迁移回滚在已有观测结果时拒绝删除数据。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 跑真实 HTTP、Artisan 迁移／配置缓存／路由缓存及独立 Worker。覆盖成功与 503 重试的实测耗时、超时、断连、响应超限、撤销、原有列表和单次查询、过期令牌的迟到写入、无数据迁移回滚后重跑及有数据时拒绝回滚。查询检查错误范围、无授权应用入口、无效尝试序号、参数冲突，以及真实响应标记没有出现在诊断中。

以 `c47d18ad8` 的受影响源码构建前版 Worker，新版 HTTP 将新任务交给前版 Worker 在新 schema 上完成。三库均保留成功状态与原尝试记录，新增传输诊断为空。该结果覆盖共享数据库和同机独立进程滚动交接。生产跨主机和托管队列保留各自验收。发送包契约、严格 Clippy、格式与三语文档随批验证。
