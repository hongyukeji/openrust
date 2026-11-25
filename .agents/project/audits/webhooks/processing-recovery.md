# Webhook 已入列任务丢失后的补派

补 WH-03 中队列已确认但记录未完成的恢复。保留固定 Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` Worker 的队列尝试、退避和时限职责。在接收记录侧，沿用[持久派发意图](dispatch-recovery.md)和同一个数据库处理租约，以隔离队列丢失、响应不确定和业务处理退出。队列可见性和领域租约具有独立期限。等待时间需考虑正常积压和退避。[Laravel Worker 固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) · [AWS SQS 可见性超时](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-visibility-timeout.html)

仅对显式启用 `recover_processing` 的 profile 保留队列确认时间与补派预算，快照与接收记录在同一事务提交。恢复进程以数据库时钟处理逾期记录，并检查当前范围、业务终态、活跃租约和延期时刻。首次已确认入列不消耗补派预算；后续在条件领取时扣减，涵盖补派进程退出或队列响应不明确。达到预算后停止自动补派，仍允许等待中的有效任务处理记录。终态跟踪行由后续实际扫描按批清除，预演只读。迁移拒绝在有跟踪记录时回滚新增列。

## 验收

基于隔离源码 `4f19d7c29`、本任务已验批次至 `4ea7bdafb` 及本批变更，构建守卫固定 Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 分别实测 1 个新增 HTTP、2 次独立 Redis 8.10.3 Worker、迁移及多次真实 Artisan。使用隔离队列执行 `queue:clear` 模拟已入列任务丢失，通过实际进程退出留下业务租约，在隔离测试库调整派发时刻和租约时刻。三库验证活跃租约跳过、过期后补派、两恢复进程竞争、预算耗尽不再补派、最后一个任务仍可处理、预演只读、终态跟踪清理和迁移回滚保护。随后三库复验补派已进入 Redis、恢复进程在确认前退出的窗口，确认该次领取已消耗预算。SQLite 还通过具名数据库队列路径。

三库各完成 136 次公开路由 HTTP、83 次独立 Worker 及其余接收回归。Webhook Client 包测试、双特性 workspace/all-targets 严格 Clippy、全仓格式、三语公开示例、文档站浏览器和已有包发现、签名、接收与许可回归已通过。日志保留于 `target/tmp/webhooks-processing-recovery`。

跨后端持久业务延期、耗尽后的运维重放和诊断、真实多主机及托管队列仍待验收。WH-03 继续开放。
