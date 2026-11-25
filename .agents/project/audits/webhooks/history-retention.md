# Webhook 投递与发布历史回收

`DeliveryStore::prune_history` 和 `webhook-server:prune --history=delivery|publication` 回收已经释放幂等键的历史记录。独立留存期限从键到期时间起算，业务保留和仍存在的键映射排除该记录。每页最多扫描 500 条，按持久 ID 分页，报告扫描、候选与实际删除数量。预演不写库。借鉴 Laravel 固定基线按有界批次查询并删除符合条件的记录。 [Laravel 固定 MassPrunable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/MassPrunable.php)

投递记录还有尝试或重放审计时不得删除。共享正文仍在时，回收从写库检查既有发布回执中的首次投递 ID，保护共享正文回收所需的引用。回执固定且新发布使用独立 ID，因此通过该引用检查的历史记录不会被后续发布重新引用。最终删除再次核对范围、时间、业务保留、键映射、尝试和重放。发布回执需先清正文并释放键，删除后历史消息 ID 不再返回诊断。工作进程旧载荷仍使用已删除的旧 ID，不会寻址续用旧键得到的新记录。

[重放审计保留期](replay-retention.md)已在后续批次接入。普通 CONNECT 代理及真实跨主机部署继续留在账本。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均通过迁移、配置与路由缓存、真实 HTTP、Artisan 和独立 Worker。隔离 SQL 回拨留存时钟，验证键到期后的独立期限、业务保留、跨范围、尝试和重放依赖、共享正文尚在时保护原投递、正文释放后的清理、预演与游标。两次并发清理只删除一次。真实旧 Job 在历史投递删除且同键新投递登记后由独立 Worker 消费，新投递仍未领取。SQLite 的原有留存、幂等到期和重放回归通过。Rust 1.94.0 格式检查、发送包九项测试、六组三语示例与受影响目标严格 Clippy 通过。
