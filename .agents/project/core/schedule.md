# `openrust::schedule` 当前事实

- Scheduler 随 `QueuePlugin` 注册，复用同一 `Db` 与 `Queue`。应用在 `QueueMigration` 中获得 `schedule_runs` 唯一 slot 表。
- `QueuePlugin::schedule_interval` 注册 1 秒至 30 天的整秒间隔；`schedule_daily_at` 注册 UTC 小时/分钟；`schedule_cron` 注册五字段 cron 和显式 IANA 时区，默认 UTC。无效语法/时区在注册时拒绝，补跑上限为每 tick 1–100。
- 每次 tick 读取数据库 UTC 时钟。间隔 slot 按 Unix 秒向下取整，每日 slot 按 UTC 日期计算。新 slot 记录和 job 派发在一个数据库事务内提交；主键 `(schedule_key, slot_key)` 使多个实例只能提交一份 job，冲突时排队插入随事务回滚。
- `schedule run --once` 执行一次 scan；`schedule run` 按 `[queue].poll_interval_millis` 持续扫描，Ctrl-C/SIGTERM 在当前 scan 结束后退出；`schedule list` 列出当前进程配置的条目。
- cron 的 DST gap 跳过；fold 的重复本地分钟映射到同一个 wall-slot key，只派发一次。默认跳过停机期间的旧触发；`CatchUp` 每 tick 有界推进，初次启用或计划版本变更时不回填历史。版本水位以表达式和 IANA 时区摘要隔离；策略变化不改变计划版本。scheduler 集群节点应运行同一构建产物，以使用相同静态 tzdb。
- 新 job 与 slot 记录在同一事务提交，多个进程共享数据库只提交一次。PostgreSQL 18 与 MySQL 8.4 的真实子进程测试覆盖同刻竞争、计划版本变更、真实 DST 槽和跨 26 小时的精确补跑；原有 interval 进程测试继续覆盖提交后进程退出和 SIGTERM。生产负载与完整集群验收仍待 M4/M6 门禁。

F1 的 cron/IANA/DST 与有界补跑已实现；M4 其余交付及完整集群门禁仍按 `todo.md` 和 [ADR-037](../decisions/adr-037-queue-worker-lifecycle.md) 推进。
