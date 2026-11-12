# 06-05 调度器暂停与恢复

## 固定基线

对照固定 Laravel [Pausing Scheduled Tasks](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#pausing-scheduled-tasks)、[SchedulePauseCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/SchedulePauseCommand.php)、[ScheduleResumeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleResumeCommand.php) 和 [SubMinuteSchedulingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/Scheduling/SubMinuteSchedulingTest.php)。参考固定 [Goravel 调度入口](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/console/run_command.go)。固定 Goravel 和 openrust-v0.x 未找到相应暂停接口。

## 实现与 Rust 适配

- `schedule:pause` 将 `illuminate:schedule:paused` 写入应用默认缓存，`schedule:resume` 和别名 `schedule:continue` 删除该键。缓存错误返回失败，状态操作成功后派发 `SchedulePaused` 或 `ScheduleResumed`。调度配置的缓存用于选举和重叠锁。
- `evenWhenPaused` 允许指定任务通过暂停检查，应用维护门禁仍独立适用。暂停由 `Schedule::configure` 中的 `doNotAllowPausing` 按应用关闭。关闭后暂停命令失败，恢复仍可清除已有暂停。
- 子分钟调度在每轮读取缓存状态，恢复可作用于当前运行。正在执行的任务可完成。暂停、条件过滤和单机选举跳过都派发 `ScheduledTaskSkipped`，事件持有调度注册完成后的任务引用 `Arc<Event>`。Rust 通过同一任务的共享引用避免复制回调和配置。
- 当前交付覆盖暂停及恢复。调度组、属性继承和其余生命周期事件继续单独验收。

## 验收

- 独立 Artisan 进程在文件和 Redis 默认缓存上验证暂停、恢复、别名、任务例外、应用隔离、禁用、缓存失败与类型化事件。文件缓存的子分钟测试在任务运行时暂停，确认当前任务完成，同一轮恢复后普通任务继续执行。维护门禁仍可跳过暂停例外。
- 三语注册示例与实际编译执行的测试夹具一致。
- Rust 1.94.0 的格式、全工作区严格 Clippy 和锁定离线全量测试通过。实际 SQLite、MySQL 8.4 和 Redis 8 共 590 项通过、7 项忽略。最终调度夹具 8 项通过、1 项忽略，连续 3 轮并行通过。
- 三语站点构建和 12 项浏览器导航检查通过，其中 6 项检查暂停小节及注册示例。

[English](../../../../docs/en/digging-deeper/scheduling.md#pausing-scheduled-tasks) · [简体中文](../../../../docs/zh-Hans/digging-deeper/scheduling.md#pausing-scheduled-tasks) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/scheduling.md#pausing-scheduled-tasks)
