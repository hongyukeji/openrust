# Webhook 跨队列后端业务延期

本批补 WH-03、WD-02 与 WH-V19，沿用固定 Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` 的原生队列尝试、超时与失败行为，借鉴事务发件箱处理接收数据库与队列分开部署的提交窗口。复用已有接收处理租约、延期记录及持久派发意图。同库原子延期入口仍可独立使用。[Laravel Worker 固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) · [AWS 事务发件箱](https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html)

可恢复派发的具名处理器可选择 `register_with_database_recovery_and_deferred_processing`。业务延期、处理尝试和具名队列快照在一个短事务提交。已存在的跟踪行通过条件派发令牌隔离新旧周期，保留可选丢失补派余额。新的业务延期继续使用独立的原生任务尝试预算，领域延期次数及总期限继续累计。提前唤醒同事务调整下次处理时间和派发意图。恢复扫描另行派发；已到总期限且没有活跃租约的跟踪记录可直接终止。预演不终止记录。已确认入列的唤醒仍保留期限跟踪，以便在消息丢失且未启用补派时按期终止。普通派发意图保持既有队列快照格式，延期模式只保存在接收记录的延期快照中。此批复用既有迁移和依赖。

## 验收

在隔离源码中以 `4f19d7c29` 和本任务已验批次至 `0407fd2a5` 加本批变更进行验收，守卫固定 Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均使用隔离 Redis 8.10.3 和独立进程。每库实测新增 3 个 HTTP、5 次 Worker，覆盖正常延期、到期恢复、两个离线唤醒进程竞争、离线派发失败与恢复、业务预算和原生任务尝试分离、已入列唤醒清除后在队列仍离线时到期终止。部分到期分支通过修改隔离测试库时刻触发。Redis 实测暂停旧派发确认，待新 Worker 先提交延期再放行，核对后续派发意图和时间仍完整。SQLite 也通过具名数据库队列路径。

三库各通过 136 个公开路由 HTTP、83 次独立 Worker 和同库延期、受控唤醒、去重、处理租约、接收恢复及附件等回归。Webhook Client 包测试、全仓格式、启用两个 Webhook 特性的 workspace/all-targets 严格 Clippy、三语公开示例、文档站浏览器、签名、包发现和许可证检查通过。首轮编译暴露事务闭包对存储的借用问题，改为在闭包中持有独立的存储句柄后复验。日志保留于 `target/tmp/webhooks-deferred-recovery`。

耗尽后的运维重放见[后续批次](manual-replay.md)。实际多主机、托管队列及真实领域幂等消费者待后续验收。WH-03 继续开放。
