# Webhook 无签名查单提示

本批补 WD-01 的无签名查单入口。固定 [Spatie SignatureValidator](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/SignatureValidator/SignatureValidator.php) 的布尔签名接口保留。接收方必须显式注册具名提示接收器。默认验证器把存在的签名头交给原验签流程，包含空头；Standard 任一协议头存在也交给原流程。自定义协议可指明其签名字段。提示单独返回 202，可信签名成功和签名拒绝继续各自原有路径。

内置适配器先确认 profile 与可信范围，再用原生缓存 RateLimiter 在解析目标前按范围和 profile 计数。策略只能返回同一范围下已登记的本地任务，未知目标不入库，应用统一返回 202。提示记录和原生任务在同一数据库事务提交。记录仅保存提示 ID、本地任务 ID、范围、profile、修订、状态及数据库时间截止。队列仅保存提示 ID 和修订。处理器有独立类型，查单前再次核对任务注册和归属。数据库只允许一次领取，过期任务和重复任务跳过。队列的单次尝试及 15 秒超时限制查单预算。供应商响应验证由领域消费者负责，原通知始终保持无签名提示身份。

隔离源码为 `4f19d7c29` 加本任务已验变更至 `efb1746ea` 及本批文件，Rust 1.94.0 和项目 build_guard。夹具覆盖未启用时的缺签名、空签名、错误签名、Standard 头不全、未知任务、错误范围、队列表故障、旧提示过期、任务撤销、重复任务、HTTP 重启后的限额和同一 profile 的合法签名。提示记录和可信收件分别核对行数及实际 Worker。日志位于 `target/tmp/webhooks-hint`。

仍须补齐可信及拒绝记录的有限验证依据、提示记录留存及超时终态回收、共享缓存的跨主机、真实审核方查单和通用跨队列恢复。WD-01 与 WH-03 整体保持开放。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过本批 12 个真实 HTTP、5 次独立 Artisan Worker。最终每库复验 186 个真实 HTTP、125 次 Worker，另执行唤醒及保留期 Artisan。三库迁移回滚再迁移、旧签名／接收回归、许可、全仓格式、两 Webhook feature 的 workspace/all-targets 严格 Clippy、三语示例和站点浏览器均通过。限流重启使用同一隔离应用目录内的文件缓存；共享远端缓存和跨主机拓扑待相应门禁。验收应用、隔离数据库和本批站点输出已清理。
