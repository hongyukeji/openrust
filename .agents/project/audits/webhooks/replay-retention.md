# Webhook 重放审计保留期

`DeliveryStore::prune_replay_audit` 及 `webhook-server:prune --replay-audit` 提供按范围预演和分批清理。原投递和重放投递均须进入终态、请求快照已清理、且各自幂等键到期均超过指定期限。任意一条投递的业务保留都会排除该审计。每批最多 500 条，先处理最早登记的候选。沿用 Laravel 固定基线的有界批次清理职责。 [Laravel 固定 MassPrunable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/MassPrunable.php)

清理事务按 ID 顺序锁定原投递与重放投递，复用尝试清理的保留锁定，并在删除前重新检查两条父记录。清理移除重放关系及其中的操作者元数据，投递历史由独立命令清理。原快照已经清理，删除关系不会使此前已耗尽的原投递再次取得重放资格。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均通过迁移、配置和路由缓存、真实 HTTP、Artisan 与独立 Worker。验证待发重放及留存期限排除、两侧保留和跨范围隔离、限制参数及预演、实际清理和原、重放历史的独立回收。测试在另一个事务锁定原投递并写入保留，在清理开始后提交，审计得到保留。解除后两次并发清理只移除一次；原快照已清理的投递仍无法重放。SQLite 的既有留存和重放回归通过。Rust 1.94.0 格式检查、发送包九项测试、三语六组示例及受影响目标严格 Clippy 通过。

普通 CONNECT 代理和真实跨主机部署仍在账本。
