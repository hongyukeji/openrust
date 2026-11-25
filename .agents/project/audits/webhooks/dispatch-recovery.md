# Webhook 接收提交后的派发恢复

补 WH-03 的接收跨后端派发窗口。固定 Spatie Client `ff886d1bae33d35ad98b6c91403f280df158129d` 的默认流程先保存接收记录，再构造任务、清理异常和派发，然后响应。Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` 的队列遵守事务提交后派发设置。OpenRust 保留两种原有接收方式，新增 profile 级 `recover_dispatch`。该选项默认关闭，与同库原子入队互斥。可靠模式采用事务发件箱做法，使用已有的数据库、Artisan、原生队列和 Worker 的接收租约。[Spatie Processor 固定源码](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookProcessor.php) · [Laravel Queue 固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Queue.php) · [AWS 事务发件箱模式](https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html)

接收记录、已有可选去重键和派发意图一并提交。快照只保存具名连接、队列、Job 和原生 Job 选项。Worker 从原数据库加载可信记录。恢复命令按可信 scope 和数据库连接扫描，最多处理 500 条；预演只读。实际派发用数据库条件更新取得 60 秒租约，等待队列确认最多 15 秒，再用令牌移除派发意图。失败或退出保留意图。内建进程内队列和 failover 连接拒绝用于该模式。Redis 接受任务后，退出可能导致重复入列，接收 Worker 复用同一领取及终态机制，业务副作用仍依赖消费者幂等。迁移拒绝回滚仍有待派发意图的表。

## 验收记录

隔离源码基于 `4f19d7c29` 和本任务已验批次至 `53718e11c`，叠加本批变更。通过构建守卫使用 Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均连接隔离的 Redis 8.10.3 实例，队列键按测试批次隔离。各库通过本批 5 个真实 HTTP 和 2 次独立 Worker。验证接收器在事务提交后、响应前退出，重复请求沿用原接收记录，两个 Artisan 恢复进程只有一个完成派发。验证队列不可用、预演、范围隔离、路由快照、内建 null 与 failover 拒绝，以及有待派发记录时的回滚保护。Redis 已入列后使 Artisan 进程退出，使用隔离测试库将租约手动置为到期后重试，确认队列中存在两个任务，但业务处理器只执行一次。

SQLite 另通过独立具名数据库队列路径。三库分别完成 136 个公开路由 HTTP、83 次独立 Worker 及租约、去重、延期、无签名提示、验签审计、附件与保留期回归。Webhook Client 包测试、全仓格式、启用两个 Webhook 特性的 workspace/all-targets 严格 Clippy、三语公开示例、文档站浏览器及包发现、签名、接收和许可证检查均通过。

单机验证以隔离项目源码和 Redis Docker 实例进行，接收器、恢复命令和 Worker 均为独立进程。首次矩阵触及项目磁盘停线，由构建守卫结束。释放项目编译产物后重编译。随后夹具给无认证 Redis 配了空密码，又将新模式 Worker 的队列断言写成旧连接。两项夹具已修正并复跑。原日志保留于 `target/tmp/webhooks-dispatch-recovery`。

入列后的消息丢失见[后续补派批次](processing-recovery.md)。跨后端业务延期、托管队列、真实多主机及扩缩容仍待验收。现有普通派发、同库原子入队和已有接收记录兼容回归仍单独执行。WH-03 继续开放。
