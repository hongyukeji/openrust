# Webhook 幂等键到期

`DeliveryStore::prune_idempotency_keys` 和 `webhook-server:prune --idempotency=delivery|publication` 提供按范围的分批到期和预演。直接投递从请求快照清理时间起算，发布从共享正文清理时间起算。业务保留中的记录排除在外。每批 1 至 500 条，以清理时间和 ID 排序。借鉴 Stripe 的旧键清理后重新登记机制，以及 Svix 的请求键和持久消息身份区分。 [Stripe 幂等](https://docs.stripe.com/api/idempotent_requests) · [Svix 幂等](https://docs.svix.com/idempotency)

清理和登记串行使用同一幂等键槽。清理在单个事务内移除键映射、更新旧记录到期时间，并复核留存时钟和业务保留。旧记录尚未采用独立映射时以旧 ID 推导键槽，再阻止后续登记回退到该旧 ID。批次也会标记已清理、没有独立键的投递元数据，计数表示已到期记录。历史 ID、状态和尝试仍可诊断，队列旧载荷继续指向旧投递。新登记使用新物理 ID；直接投递的应用业务消息 ID 继续由调用者决定。

迁移在存在已到期记录时拒绝回滚。留存迁移在幂等到期索引仍存在时提示依次回滚。启用清理前先迁移数据库，升级写入和清理进程。[投递与发布历史回收](history-retention.md)已在后续批次接入。重放审计保留、普通 HTTP CONNECT、接收侧保留与跨主机部署继续跟踪。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过迁移、配置与路由缓存、真实 HTTP、Artisan 和独立 Worker。覆盖业务保留、预演、范围及上限、从快照清理时间计时、并发清理与同键登记、旧确定性 ID 的回退限制、共享发布重新扇出、迁移回滚与历史诊断。旧格式无映射行用隔离数据库按旧 ID 布局构造；上一批实测过旧二进制的升级兼容。旧 Job 取自原生队列，在新投递登记后由另一个 Artisan Worker 消费，旧投递历史保持，新投递仍等待领取。三库出站留存回归及 SQLite 既有扇出与重放通过。Rust 1.94.0 格式检查、发送包九项测试、三语六组示例和受影响目标严格 Clippy 通过。
