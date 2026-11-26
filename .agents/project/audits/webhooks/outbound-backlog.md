# 出站恢复积压预览

`DeliveryService::inspect_recovery_backlog` 与恢复扫描共用候选条件、范围过滤及排序。预览从主写库只读取 `available_at`，最多 `limit + 1` 条。候选包含可唤醒的投递，以及达到截止或预算、可由扫描收尾的投递。`oldest_available_at` 是候选中最早的已保存可用时间。它可以用于观察积压，无需读取 URL、正文、凭据、投递 ID 或异常详情。

`webhook-server:recover --scope=<scope> --pretend --backlog` 返回同一份 JSON。必须显式预演。无需连接队列，不建立唤醒租约、不写尝试或改变投递状态。旧预演和实际恢复的结果字段继续保留。

## 验收

SQLite、PostgreSQL 18.7 及 MySQL 8.4.13 搭配原生数据库队列，后二者另搭配 Redis 8.10.3。五组使用真实 HTTP 创建候选，Artisan 复验截断、最早时间、空范围、活跃唤醒窗口、到期收尾及队列不可用时只读预览。两个 Scheduler 在共享主写库协调唤醒，独立 Worker 实际投递并由 HTTP 收件端验签。队列丢失、退避、进程退出、有限补派和迁移回滚同次回归。

常规三库发送套件复验签名及原始字节、事务回滚、领取、重试预算、撤销、超时、崩溃恢复、Builder、事件及自定义 Job。Rust 1.94.0 的包测试、包及应用严格 Clippy、格式、三语构建和浏览器检查均通过。
