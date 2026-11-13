# 06-05 任务调度

依赖 03-06 Redis 与共享缓存锁、06-03 Queue，均已提交。应用在控制台路由注册作用域配置 Schedule。Kernel 为每次 Artisan 应用装配独立调度表，支持回调、Artisan、外部程序和队列任务。命令和参数独立传入。队列任务重用 QueueManager，每次产生新的任务 ID。

对照 Laravel v13.40.0 的 [Schedule](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Console/Scheduling/Schedule.php)、[ScheduleRunCommand](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Console/Scheduling/ScheduleRunCommand.php)、[CacheEventMutex](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Console/Scheduling/CacheEventMutex.php) 与 [CacheSchedulingMutex](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Console/Scheduling/CacheSchedulingMutex.php)。参考固定 Goravel 的应用调度登记与命令分派。核对 `openrust-v0.x` 的 `schedule.rs`、`schedule_calendar.rs`、`schedule_coordination.rs`，沿用 UTC 时点转换到任务时区及跨进程竞争用例。当前用 Croner 解析五字段 cron，支持日期与星期的 OR 语义。调度只评估本分钟，互斥由共享缓存实现。

默认重叠锁为 1,440 分钟，成功与失败均按持有者释放。单实例选举按事件与 UTC 分钟原子添加键，缓存保留一小时。子分钟任务沿用已取得的本分钟选举结果。`schedule:interrupt` 发布当前分钟的缓存标记。`schedule:work` 在同一进程中逐轮执行，并在收到信号后等待当前任务结束。

## 验收

- Rust 1.94.0 隔离候选工作区，锁定依赖测试 477 项通过。两个子进程 fixture 由集成测试启动。严格 Clippy 和格式检查通过。
- 实际 Artisan 进程验证应用与事件时区、日期与星期 OR、环境及条件过滤、命令参数、失败报告、继续执行、退出码和队列任务 ID 更新。实测根应用 `./artisan schedule:run`、`schedule:list` 和 `inspire`。
- Redis 8 实际跨进程验收。重叠时竞争者跳过，失败后能重新取得锁。同一分钟仅一台运行，下一分钟可重新选举。子分钟任务收到共享中断后停止。Unix SIGTERM 下常驻进程完成当前任务并退出。
- 三语文档构建通过。通用浏览器检查 12 项，调度页面在两个部署路径共 6 项。三语共同 Rust 示例已编译，并通过实际 `schedule:list` 验证表达式和时区。

本单元已交付 `schedule:run/list/work/interrupt`，并完成暂停和恢复、输出邮件、清锁及手工测试。后续计划为 06-06 Mail 与 Notifications。 [暂停和恢复验收](scheduler-pause.md) · [输出与调度命令验收](scheduler-output-background.md)
