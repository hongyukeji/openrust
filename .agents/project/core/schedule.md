# `openrust::schedule` 当前事实

- Scheduler 随 `QueuePlugin` 注册，复用同一 `Db` 与 `Queue`。应用在 `QueueMigration` 中获得 `schedule_runs` 唯一 slot 表。
- `QueuePlugin::schedule_interval` 注册 1 秒至 30 天的整秒间隔；`schedule_daily_at` 注册 UTC 小时/分钟。名称键稳定且唯一，Job 必须在同一插件中显式注册；配置错误在启动时拒绝。
- 每次 tick 读取数据库 UTC 时钟。间隔 slot 按 Unix 秒向下取整，每日 slot 按 UTC 日期计算。新 slot 记录和 job 派发在一个数据库事务内提交；主键 `(schedule_key, slot_key)` 使多个实例只能提交一份 job，冲突时排队插入随事务回滚。
- `schedule run --once` 执行一次 scan；`schedule run` 按 `[queue].poll_interval_millis` 持续扫描，Ctrl-C/SIGTERM 在当前 scan 结束后退出；`schedule list` 列出当前进程配置的条目。
- 本版不支持 cron、IANA 时区、DST 或补跑错过的 daily slot。注册表由各应用进程本地配置；持久化 slot 只记录已触发位置，不提供任意 schedule UI 或任务状态查询。
- PostgreSQL 18 与 MySQL 8.4 的真实双进程同步竞争测试确认只插入一条 schedule run 和一条 job；另验证已提交 slot 后进程异常退出仍不重复派发，以及常驻命令收到 SIGTERM 后正常退出。生产拓扑下的 scheduler 集群验收仍待M4 专项与 v1 最终集群门禁。

V1 的 cron/IANA/DST 与有界补跑已列为 [F1 日历调度](../plans/v1-foundation-completion.md#f1-日历调度) 必交项，当前代码仍仅实现上述 UTC 基础频率，后续复用同一调度器和数据库时钟。
