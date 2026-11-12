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

## 防止重叠与单机执行

`withoutOverlapping()` 在执行期间持有缓存锁，任务完成或报错后释放。默认租期为 1,440 分钟，可用 `withoutOverlappingFor(minutes)` 调整。`onOneServer()` 为每个事件在每分钟选出一个执行者。回调或队列任务启用互斥前，需要用 `name` 设置稳定且唯一的名称。

跨实例互斥要求所有执行者使用同一共享缓存存储及命名空间。可以使用应用默认的共享存储，或用 `schedule.useCache("redis")` 指定。

## 运行调度器

在系统调度器中每分钟调用 `./artisan schedule:run`，也可以用 `./artisan schedule:work` 常驻运行。常驻进程收到 SIGTERM 或 Ctrl+C 后会完成当前任务。用 `./artisan schedule:list` 查看表达式和下一次执行时间。`schedule:run --whisper` 隐藏没有任务运行时的提示。

有子分钟任务时，`schedule:run` 持续到当前分钟结束。`./artisan schedule:interrupt` 通过所选缓存存储通知当前分钟的重复循环退出。任务错误交给异常报告器，后续到期任务继续执行，调度命令处理完后成功退出。调度基础设施发生错误时，命令以失败状态退出。
