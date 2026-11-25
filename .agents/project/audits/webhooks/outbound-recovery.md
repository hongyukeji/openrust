# Webhook 出站定时恢复

## 实现与边界

新增 `webhook-server:recover` 和应用级 `DeliveryService::recover_due`。应用显式配置范围与调度频率。命令走已有 Laravel 风格的 Artisan Scheduler，把唤醒任务交给原生 Queue，Worker 仍通过已有投递租约领取和发送。共享数据库协调每条投递的唤醒，Scheduler 可在多个进程运行。[固定 Laravel Scheduler 基线](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md)

持久记录添加可空的 `next_wake_at`。调度器在推送任务前按数据库时钟原子设置 60 秒窗口，循环有候选数量及派发时间预算。一次扫描至多选择 1–500 条，单次 Queue 派发至多等待 15 秒，整批派发时间预算 30 秒。`--pretend` 仅计数，失败汇总使命令返回非零。Worker 领取与提交结果时清除唤醒窗口，新的尝试仍服从持久退避与截止时间。过期或耗尽记录按原事务规则收尾。迁移可独立添加和撤除列，既有投递和尝试历史保留。

一次推送失败、进程在推送前退出，或已推送任务从队列消失时，下一次合格扫描可补派。队列确认和数据库写入各有持久边界，长队列积压可能造成重复唤醒，统一投递领取继续控制实际发送。恢复不增加尝试预算，不重置原幂等键。文档示例让每个生产应用选择自己的范围、队列与调度配置。[AWS Transactional Outbox](https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html)

## 验收

使用隔离源码快照和已提交的 Webhook 基线 `72977ed07`，叠加本批文件。日志位于 `target/tmp/webhooks-outbound-recovery/`。隔离夹具用真实 HTTP、Artisan `schedule:list`／`schedule:run` 和独立 Worker。第二个调度进程有独立应用目录和配置缓存，数据库和队列指向同一批次实例。覆盖预演与跨范围、两个调度器竞争、提交后未入队、任务丢失、退避等待、监听器失败、推送前崩溃、发送进程崩溃后重领及迁移回滚重建。


SQLite、PostgreSQL 18.7、MySQL 8.4.13 的数据库队列，以及 PostgreSQL／MySQL 搭配 Redis 8.10.3 队列，共五组通过。分别通过无效的数据库队列表和拒绝连接的本地 Redis 地址触发真实队列派发失败；恢复队列后补派并通过签名核验。到期边界使用隔离数据库的记录时刻推进，以确定性验证恢复条件。SQLite 接收回归的 136 个 HTTP 请求、83 次独立 Worker，以及八个包测试、三语六组示例、全仓格式与严格 Clippy 通过。正式多主机网络、缓存和负载均衡、缩扩容及后端实际故障转移仍待验收。

[自定义 Job 接缝](custom-job.md)已在后续批次接入。重放、端点管理和其他 WH-04／WH-05 门禁继续验收。
