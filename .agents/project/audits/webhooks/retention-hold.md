# Webhook 出站业务保留

`DeliveryStore::set_retention_hold` 和 `webhook-server:hold` 支持按范围设置或解除现有记录的保留标记。重复设置或解除返回无变化，跨范围和不存在的记录也返回无变化。投递保留排除其快照与尝试，并通过现存快照保护引用的共享正文。消息保留仅作用于该消息的共享正文。标记没有默认过期时间；解除后仍按普通留存期限判断。诊断显示当前 `retention_hold_at`。借鉴 S3 将独立期限和显式保留分设的机制。这个数据库标记约束 OpenRust 的清理入口，底层数据库管理、备份和合规审计仍由应用和部署负责。 [AWS S3 Object Lock 保留说明](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html)

快照、共享正文在更新时复核保留标记。尝试清理与保留变更使用同一投递行锁，SQLite 先进入写事务。清理只删除锁定后仍符合条件的候选。迁移在存在活动保留时拒绝回滚。启用前需迁移数据库并升级运行清理的实例。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均通过真实 HTTP、Artisan 与独立 Worker。验证范围隔离、并发重复操作只变更一次、预演与实际清理跳过保留、解除后按原期限清理、活动状态拒绝回滚。并发测试在独立数据库事务中先锁定投递并写入保留，启动尝试清理后才提交，验证尝试仍在。原重放、共享正文、分页和幂等回归通过。Rust 1.94.0 格式检查、发送包九项合约、三语六组示例及受影响目标严格 Clippy 通过。

[幂等键与持久 ID](idempotency-identity.md)已分离。[幂等键到期释放](idempotency-expiry.md)已在后续批次接入。保留操作历史、独立接收侧保留、普通 CONNECT 代理和真实跨主机部署仍沿账本跟踪。
