# 任务调度

## 定义调度

在 `routes/console.rs` 的 Artisan 注册作用域中登记调度任务。在应用现有的 `register()` 函数中调用 `Schedule::configure`。

```rust
use openrust::{console::ConsoleError, support::facades::Schedule};

pub(crate) fn register() -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        schedule
            .command("inspire", [])
            .dailyAt("08:30")?
            .timezone(chrono_tz::Asia::Shanghai)
            .withoutOverlapping();

        Ok(())
    })
}
```

用 `call` 登记异步回调，用 `command` 调用已注册的 Artisan 命令，用 `exec` 运行外部程序，用 `job` 分派队列载荷。程序与参数分别传入。`exec` 从应用根目录启动。回调接收应用和命令输出对象。每次调度分派都生成新的任务 ID。

用 `cron` 设置五字段表达式，用 `dailyAt` 设置每日的本地时间，用 `everyMinute` 或 `everyTenSeconds` 等子分钟频率运行重复任务。调度时区依次取 `app.schedule_timezone`、`app.timezone`、UTC。用 `timezone` 覆盖单个任务的时区。用 `environments`、`when` 和 `skip` 限制执行条件。夏令时切换时，同一本地时间可能出现两次，也可能被跳过。

### 维护模式 {#maintenance-mode}

应用进入[维护模式](../getting-started/configuration.md#maintenance-mode)后，默认跳过调度任务。需要在维护期间执行的任务，可在 `Schedule::configure` 中调用 `evenInMaintenanceMode()`。

```rust
schedule.command("inspire", []).evenInMaintenanceMode();
```

子分钟调度检测到维护模式后，会在本次运行余下的时间继续跳过普通任务。执行 `./artisan up` 后，下一次 `./artisan schedule:run` 可以重新执行普通任务。

### 暂停调度任务 {#pausing-scheduled-tasks}

运行 `./artisan schedule:pause` 暂停调度器，运行 `./artisan schedule:resume` 或别名 `./artisan schedule:continue` 恢复。暂停状态保存在应用的默认缓存中。多个进程需要共享暂停状态时，使用同一默认缓存和命名空间。调度互斥锁可通过 `schedule.useCache(...)` 选择缓存。

在 `Schedule::configure` 中调用 `evenWhenPaused()`，可让指定任务在暂停期间仍符合执行条件。

```rust
schedule.command("inspire", []).evenWhenPaused();
```

子分钟调度在同一轮中可感知暂停和恢复，已经开始的任务会继续执行。可在 `Schedule::configure` 中调用 `schedule.doNotAllowPausing()` 禁用应用的暂停功能。可监听 `openrust::console::events` 中的 `SchedulePaused`、`ScheduleResumed` 和 `ScheduledTaskSkipped`。

### 调度组 {#schedule-groups}

在 `Schedule::configure` 中用 `group` 为多个任务设置共同的调度属性。创建调度组前至少设置一个属性。

```rust
schedule
    .timezone(chrono_tz::Asia::Tokyo)
    .dailyAt("17:00")?
    .group(|schedule| {
        schedule.command("inspire", []);
        Ok(())
    })?;
```

嵌套组继承外层设置。嵌套组和单个任务都可覆盖时区、频率等设置。过滤条件会累积。临时设置的属性仅应用于下一次任务注册。退出调度组后恢复外围设置。

## 防止重叠与单机执行

`withoutOverlapping()` 在执行期间持有缓存锁，任务完成或报错后释放。默认租期为 1,440 分钟，可用 `withoutOverlappingFor(minutes)` 调整。`onOneServer()` 为每个事件在每分钟选出一个执行者。回调或队列任务启用互斥前，需要用 `name` 设置稳定且唯一的名称。

跨实例互斥要求所有执行者使用同一共享缓存存储及命名空间。可以使用应用默认的共享存储，或用 `schedule.useCache("redis")` 指定。

## 运行调度器

在系统调度器中每分钟调用 `./artisan schedule:run`，也可以用 `./artisan schedule:work` 常驻运行。常驻进程收到 SIGTERM 或 Ctrl+C 后会完成当前任务。用 `./artisan schedule:list` 查看表达式和下一次执行时间。`schedule:run --whisper` 隐藏没有任务运行时的提示。

有子分钟任务时，`schedule:run` 持续到当前分钟结束。`./artisan schedule:interrupt` 通过所选缓存存储通知当前分钟的重复循环退出。任务错误交给异常报告器，后续到期任务继续执行，调度命令处理完后成功退出。调度基础设施发生错误时，命令以失败状态退出。

## 事件 {#events}

从 `openrust::console::events` 导入调度事件类型，并在应用装配作用域注册监听器。示例使用自定义的 `ScheduleEvents` 监听器和将注册错误转为 `ConsoleError` 的 `console_error`。

```rust
let listener = Arc::new(ScheduleEvents::default());
for name in [
    "console.scheduled_task_starting",
    "console.scheduled_task_finished",
    "console.scheduled_task_failed",
    "console.scheduled_task_skipped",
] {
    Event::listen(name, listener.clone(), Delivery::Sync).map_err(console_error)?;
}
```

前台任务执行前派发 `ScheduledTaskStarting`。命令执行结束时派发 `ScheduledTaskFinished`，退出码非零时接着派发 `ScheduledTaskFailed`。执行抛错时，开始事件之后派发失败事件。完成和失败事件携带耗时 `Duration`，失败事件还携带异常。

任务因过滤条件、暂停、单机选举或重叠锁被跳过时派发 `ScheduledTaskSkipped`。
