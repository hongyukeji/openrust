# 接收补派积压预览

`DatabaseWebhookStore::inspect_dispatch_backlog` 从当前范围的主写库读取已就绪补派。复用恢复命令的到期、活跃租约、延期、终态和预算条件。查询最多取得 `limit + 1` 条，只投影就绪时间。报告最多计 `limit` 条，同时返回是否还有更多记录及最早可补派的 Unix 秒时间。没有符合条件的记录时最早时间为空。

`webhook-client:recover --scope=<scope> --pretend --backlog` 输出同一个 JSON 结果。预览不连接队列，不领取任务或扣减预算。原来的预演文本、实际恢复结果和退出码继续保留。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 以真实 HTTP 生成两个不同的可恢复记录，再用真实 Artisan 验证空范围、精确批次、截断、最早时间、活跃领取排除、显式预演要求和无写回。独立 Worker 复验队列故障后恢复、并发扫描竞争、服务退出及独立租约。同时回归接收路由、缓存和持久重放。包契约、应用与包严格 Clippy、格式和三语文档通过。

同一制品另复跑 SQLite 单节点，以及 PostgreSQL／MySQL 各搭配 database、Redis 8.10.3 队列与调度缓存的同主机双节点。五组实测真实 HTTP、独立 Worker、缓存、进程退出与接管、共享附件、限额和同步／异步派发。
