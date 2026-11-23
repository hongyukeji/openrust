# 任务调度

## 定义调度

在 `routes/console.rs` 的 Artisan 注册作用域中登记调度任务。在应用现有的 `register()` 函数中调用 `Schedule::configure`。

```rust
use openrust::{console::ConsoleError, support::facades::Schedule};

pub(crate) fn register() -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        schedule
            .command("inspire", [])
            .daily_at("08:30")?
            .timezone(openrust::support::Date::timezone("Asia/Shanghai").expect("known IANA time zone"))
            .without_overlapping();

        Ok(())
    })
}
```

用 `call` 登记异步回调，用 `command` 调用已注册的 Artisan 命令，用 `exec` 运行外部程序，用 `job` 分派队列载荷。程序与参数分别传入。`exec` 从应用根目录启动。回调接收应用和命令输出对象。每次调度分派都生成新的任务 ID。

用 `cron` 设置五字段表达式，用 `daily_at` 设置每日的本地时间，用 `every_minute` 或 `every_ten_seconds` 等子分钟频率运行重复任务。调度时区依次取 `app.schedule_timezone`、`app.timezone`、UTC。用 `timezone` 覆盖单个任务的时区。用 `environments`、`when` 和 `skip` 限制执行条件。夏令时切换时，同一本地时间可能出现两次，也可能被跳过。

### 维护模式 {#maintenance-mode}

应用进入[维护模式](../getting-started/configuration.md#maintenance-mode)后，默认跳过调度任务。需要在维护期间执行的任务，可在 `Schedule::configure` 中调用 `even_in_maintenance_mode()`。

```rust
schedule.command("inspire", []).even_in_maintenance_mode();
```

子分钟调度检测到维护模式后，会在本次运行余下的时间继续跳过普通任务。执行 `./artisan up` 后，下一次 `./artisan schedule:run` 可以重新执行普通任务。

### 暂停调度任务 {#pausing-scheduled-tasks}

运行 `./artisan schedule:pause` 暂停调度器，运行 `./artisan schedule:resume` 或别名 `./artisan schedule:continue` 恢复。暂停状态保存在应用的默认缓存中。多个进程需要共享暂停状态时，使用同一默认缓存和命名空间。调度互斥锁可通过 `schedule.use_cache(...)` 选择缓存。

在 `Schedule::configure` 中调用 `even_when_paused()`，可让指定任务在暂停期间仍符合执行条件。

```rust
schedule.command("inspire", []).even_when_paused();
```

子分钟调度在同一轮中可感知暂停和恢复，已经开始的任务会继续执行。可在 `Schedule::configure` 中调用 `schedule.do_not_allow_pausing()` 禁用应用的暂停功能。可监听 `openrust::console::events` 中的 `SchedulePaused`、`ScheduleResumed` 和 `ScheduledTaskSkipped`。

### 调度组 {#schedule-groups}

在 `Schedule::configure` 中用 `group` 为多个任务设置共同的调度属性。创建调度组前至少设置一个属性。

```rust
schedule
    .timezone(openrust::support::Date::timezone("Asia/Tokyo").expect("known IANA time zone"))
    .daily_at("17:00")?
    .group(|schedule| {
        schedule.command("inspire", []);
        Ok(())
    })?;
```

嵌套组继承外层设置。嵌套组和单个任务都可覆盖时区、频率等设置。过滤条件会累积。临时设置的属性仅应用于下一次任务注册。退出调度组后恢复外围设置。

## 防止重叠与单机执行

`without_overlapping()` 在执行期间持有缓存锁，任务完成或报错后释放。默认租期为 1,440 分钟，可用 `without_overlapping_for(minutes)` 调整。`on_one_server()` 为每个事件在每分钟选出一个执行者。回调或队列任务启用互斥前，需要用 `name` 设置稳定且唯一的名称。

跨实例互斥要求所有执行者使用同一共享缓存存储及命名空间。可以使用应用默认的共享存储，或用 `schedule.use_cache("redis")` 指定。

## 运行调度器

在系统调度器中每分钟调用 `./artisan schedule:run`，也可以用 `./artisan schedule:work` 常驻运行。常驻进程收到 SIGTERM 或 Ctrl+C 后会完成当前任务。用 `./artisan schedule:list` 查看表达式和下一次执行时间。`schedule:run --whisper` 隐藏没有任务运行时的提示。

要筛选列表，可为每个环境传一次 `--environment`。能在任意指定环境运行的任务都会显示，未限制环境的任务也会显示。用 `--next` 按下次运行时间排序。时间默认按 `app.timezone` 显示，可用 `--timezone` 指定 IANA 时区。列表用任务下次 cron 触发时的时区偏移转换表达式。跨日时，一个表达式可能拆成多行。转换无法保留日历规则时，列表显示原表达式。任务仍按原调度配置触发。

```sh
./artisan schedule:list --environment=staging --environment=production --next --timezone=Asia/Kolkata
```

使用 `--json` 输出数组，包含表达式、命令、描述、下次触发时间、时区、互斥锁状态、重复间隔、适用环境和单服务器标记。空调度表输出 `[]`。

```sh
./artisan schedule:list --json --next --timezone=UTC
```

终端列表显示相对触发时间、重复间隔及任务是否持有互斥锁。使用 `-v` 查看带时区偏移的绝对时间和任务描述。Artisan 任务在详细模式下会显示当前二进制路径及 `artisan`。用 `--no-ansi` 去掉终端颜色。JSON 输出始终是纯文本。

```sh
./artisan schedule:list -v --no-ansi
```

有子分钟任务时，`schedule:run` 持续到当前分钟结束。`./artisan schedule:interrupt` 通过所选缓存存储通知当前分钟的重复循环退出。任务错误交给异常报告器，后续到期任务继续执行，调度命令处理完后成功退出。调度基础设施发生错误时，命令以失败状态退出。

用 `./artisan schedule:test --name=reports:daily` 在前台立即运行选定任务。在交互式终端中可省略 `--name`，从已登记任务中选择。用 `./artisan schedule:clear-cache` 清理已登记任务的重叠锁，包括共享 Redis 中的锁。

### 测试调度任务 {#testing-scheduled-tasks}

调度器使用 `TestApp` 时钟判断任务是否到期，并计算 `schedule:list` 显示的下次运行时间。显式设置的 `Schedule::use_clock` 优先。分钟内重复任务正在生效时，`schedule:list` 显示下一次秒级触发时间。分钟内重复任务仍按实际经过时间计时。详见[控制测试时间](../testing/mocking.md#interacting-with-time)。

```rust
use openrust::{support::facades::Schedule, testing::TestApp};

#[tokio::test]
async fn runs_a_task_with_a_frozen_clock() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| {
        builder.with_commands(|_| Schedule::configure(|schedule| {
            schedule.do_not_allow_pausing();
            schedule.call(|_, output| Box::pin(async move {
                output.stdout.push_str("scheduled task ran\n");
                Ok(())
            })).every_minute();
            Ok(())
        }))
    })?;

    app.freeze_second().await?;
    app.artisan(["schedule:run"]).await
        .assert_successful()
        .expects_output("scheduled task ran");
    Ok(())
}
```

## 任务输出 {#task-output}

用 `send_output_to` 覆盖文件，用 `append_output_to` 追加。两者均捕获调度 Artisan 命令与系统可执行程序的标准输出和标准错误，包括非零退出时的输出。相对路径以应用根目录为基准。

```rust
schedule.command("reports:daily", []).send_output_to("storage/logs/daily.log");
schedule.command("reports:audit", []).append_output_to("storage/logs/audit.log");
```

用 `emailOutputTo(["ops@example.com"])` 在任务运行后发送非空输出。用 `emailOutputOnFailure(["ops@example.com"])` 仅在任务失败时发送，即使输出为空也会发送。两者使用应用邮件器。

## 任务钩子 {#task-hooks}

可为单个任务或调度组注册钩子。示例中的 `hook` 创建接收应用和控制台输出的异步回调。

```rust
schedule
    .command("fixture:hello", [])
    .name("hook-success")
    .before(hook("task-before"))
    .then(hook("task-then"));
```

调度器在执行任务前运行 `before`。`after` 与 `then` 等价。`on_success` 和 `on_failure` 按任务结果执行。任务结束后的钩子按注册顺序运行。嵌套组继承外层钩子，并可追加自己的钩子。钩子异常交给应用异常处理器报告。

用 `then_with_output`、`on_success_with_output` 或 `on_failure_with_output` 让钩子的第三个参数接收输出字节。第二个参数传入 `true` 时跳过空输出。指定输出文件时，钩子读取完整文件，追加模式也包含之前运行的内容。

### URL 探测 {#pinging-urls}

用 `ping_before`、`then_ping`、`ping_on_success` 和 `ping_on_failure` 在对应钩子发送 HTTP GET 请求。`If` 变体依次接收布尔值和 URL。请求失败会报告异常，调度继续执行。

## 后台任务 {#background-tasks}

对调度命令或可执行程序调用 `run_in_background()`。OpenRust 在同一应用二进制的独立进程中执行任务及后置钩子。父进程先运行前置钩子，可在任务仍运行时退出。与 `without_overlapping()` 一起使用时，选择文件或 Redis 等共享缓存。

子进程完成任务并释放锁后派发 `ScheduledBackgroundTaskFinished`。事件携带任务和退出码，也覆盖任务失败。应用装配时为 `console.scheduled_background_task_finished` 注册监听器。

在 Windows ARM64 上，后台子进程使用同一个应用可执行文件。命令成功、非零退出或后置钩子失败后，文件缓存和 Redis 缓存均可释放重叠锁。父进程退出后，调用方即可从其 stdout 和 stderr 读到 EOF，后台任务可以继续执行。

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

## 长任务回调

用 `Schedule::when_task_runs_longer_than(Duration, callback)` 监听运行时间严格大于阈值的已完成或失败任务。相等不触发，失败事件没有 runtime 时也不触发。回调接收任务和 Duration；不会增加另一套计时器。后台父进程完成事件与后台子进程的后续完成事件应按实际事件分别处理。
