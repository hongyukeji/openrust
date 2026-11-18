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

## 调度读取应用测试时钟

- 固定 Laravel 用 `Date::now` 确定调度起点和事件到期时间。旧版 `schedule.rs` 的 UTC 与任务时区换算继续沿用。接入现有 T-04 应用时钟，未冻结时使用系统时间，显式 `Schedule::useClock` 优先。分钟内重复任务继续通过单调时钟度量实际经过时长。 [固定 Laravel ScheduleRunCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleRunCommand.php#L84) · [固定 Laravel Event](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/Event.php#L266)
- 以 `a0d879c5f` 的干净归档验证两个并行应用的 `schedule:run` 彼此隔离，`schedule:list` 用应用时区显示下一次运行，临时冻结结束后恢复外层时间。覆盖 Unix 纪元之前、显式时钟优先，以及距分钟末 100 毫秒时的子分钟任务停止。
- `schedule:list` 在生效的重复分钟内显示下一次秒级触发。PHP 8.5.12 与 Carbon 3.8.4 直接运行固定 Laravel 的秒级舍入表达式，验证小数秒、整秒和跨分钟三个结果，与实际 Artisan 列表一致。 [固定 Laravel ScheduleListCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleListCommand.php#L264)
- Redis 8.10.3 与真实子进程复验互斥、选举、中断、清锁及关停。调度与三语示例定向共 21 项通过，1 项子进程夹具忽略。根应用的独立 `./artisan schedule:list`、`schedule:run --whisper` 和 `inspire` 通过。全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。隔离 Redis、编译和站点产物已回收，项目盘可用 100.02 GB（93.15 GiB），提交前周额度剩余 36%。日志保留于 `/private/tmp/openrust-schedule-clock.dBi58tl9`。

## 调度列表的环境筛选、排序与展示时区

- 固定 Laravel v13.40.0 的 `schedule:list` 支持重复指定 `--environment`，按任一环境匹配并保留未限制环境的任务。`--next` 按下次触发时间稳定排序，`--timezone` 指定显示时间的时区，默认读取 `app.timezone`。核对 `openrust-v0.x` 的 `schedule.rs`，旧 `schedule list` 仅显示登记项，继续沿用旧版以 UTC 时点计算的调度机制。本批在现有 Rust 调度列表接入三个选项，解析 IANA 时区，继续显示原 cron 表达式及 RFC 3339 时间。`--json`、cron 表达式的时区转换和详细终端输出仍待后续对齐。 [固定 Laravel ScheduleListCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleListCommand.php#L23) · [固定上游测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/Scheduling/ScheduleListCommandTest.php#L160)
- 以 `312befbc4` 的干净归档，通过独立 Artisan 进程验证默认登记顺序、单一及多个环境、无限制任务、同一触发时刻的排序稳定性、东京和 UTC 调度到 Kolkata 展示、无效时区失败。PHP 8.5 的日期转换给出相同的三个时刻。根应用实际执行三语共同示例。
- 锁定依赖的调度回归 19 项通过、1 项子进程夹具忽略。真实 Redis 8.10.3 跨进程互斥、选举和中断通过。Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建及 18 项浏览器检查通过。临时 Redis、隔离 Cargo 和文档产物已回收，提交前磁盘可用 99.84 GB（92.98 GiB），周额度剩余 36%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-schedule-list-options.lAgivy32`。
