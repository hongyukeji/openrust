# Task Scheduling

## Defining schedules

Register scheduled tasks in the same `routes/console.rs` scope as Artisan commands. Add `Schedule::configure` to the application's existing `register()` function.

```rust
use openrust::{console::ConsoleError, support::facades::Schedule};

pub(crate) fn register() -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        schedule
            .command("inspire", [])
            .dailyAt("08:30")?
            .timezone(openrust::support::Date::timezone("Asia/Shanghai").expect("known IANA time zone"))
            .withoutOverlapping();

        Ok(())
    })
}
```

Use `call` for an async callback, `command` for a registered Artisan command, `exec` for an external program, and `job` to dispatch a queue payload. Pass program and arguments separately. `exec` runs from the application base path. The callback receives the application and the command output. Each scheduled queue dispatch creates a fresh job ID.

Use `cron` for a five-field expression, `dailyAt` for a daily wall-clock time, and `everyMinute` or a sub-minute frequency such as `everyTenSeconds` for repeated tasks. The scheduler uses `app.schedule_timezone`, then `app.timezone`, then UTC. Use `timezone` to override it for one task. `environments`, `when` and `skip` filter execution. At daylight-saving transitions a local time can occur twice or be skipped.

### Maintenance mode {#maintenance-mode}

By default, scheduled tasks are skipped while the application is in [maintenance mode](../getting-started/configuration.md#maintenance-mode). To keep a task eligible during maintenance, add `evenInMaintenanceMode()` inside `Schedule::configure`.

```rust
schedule.command("inspire", []).evenInMaintenanceMode();
```

When a sub-minute run observes maintenance mode, ordinary tasks stay suspended until that run ends. After `./artisan up`, the next `./artisan schedule:run` can run ordinary tasks again.

### Pausing scheduled tasks {#pausing-scheduled-tasks}

Pause the scheduler with `./artisan schedule:pause`. Resume it with `./artisan schedule:resume` or its alias `./artisan schedule:continue`. The pause is stored in the application's default cache, so use a shared default cache and namespace when separate scheduler processes must see the same state. Use `schedule.useCache(...)` to select the store for scheduling mutexes.

Add `evenWhenPaused()` inside `Schedule::configure` to keep a task eligible while the scheduler is paused.

```rust
schedule.command("inspire", []).evenWhenPaused();
```

Sub-minute runners observe both pause and resume during the same run. A pause lets a running task finish. To disable pausing for an application, call `schedule.doNotAllowPausing()` inside `Schedule::configure`. Listen for `SchedulePaused`, `ScheduleResumed`, and `ScheduledTaskSkipped` from `openrust::console::events`.

### Schedule groups {#schedule-groups}

Apply shared scheduling attributes to multiple tasks with `group` inside `Schedule::configure`. Set at least one attribute before opening a group.

```rust
schedule
    .timezone(openrust::support::Date::timezone("Asia/Tokyo").expect("known IANA time zone"))
    .dailyAt("17:00")?
    .group(|schedule| {
        schedule.command("inspire", []);
        Ok(())
    })?;
```

Nested groups inherit the outer settings. A nested group or individual task can override settings such as time zone and frequency. Conditions accumulate. Attributes configured for a single task apply to its next registration. Registrations after the group resume the surrounding settings.

## Preventing overlaps and selecting one server

`withoutOverlapping()` holds a cache lock during execution and releases it on completion, including task errors. Its default lease is 1,440 minutes. Set a different lease with `withoutOverlappingFor(minutes)`. `onOneServer()` elects one runner for an event in each minute. Give a callback or job a stable, distinct `name` before enabling either mutex.

All runners must use the same shared cache store and namespace for cross-server locking. Use the application's default shared store or select one with `schedule.useCache("redis")`.

## Running the scheduler

Call `./artisan schedule:run` each minute from your process scheduler, or use `./artisan schedule:work` to keep a scheduler process running. The worker finishes the current task on SIGTERM or Ctrl+C. Use `./artisan schedule:list` to see expressions and upcoming run times. `schedule:run --whisper` hides the message when no task runs.

To filter the list, pass `--environment` once per environment. A task appears if it can run in any requested environment, and tasks without an environment restriction always appear. Use `--next` to sort by the next run. By default, times use `app.timezone`. Set `--timezone` to an IANA timezone to change the displayed timestamps. The list converts each cron expression using the timezone offset at the next cron occurrence. An expression can produce several rows when times cross a day boundary. When a conversion cannot preserve the calendar rules, the list keeps the original expression. The task itself still runs on its configured schedule.

```sh
./artisan schedule:list --environment=staging --environment=production --next --timezone=Asia/Kolkata
```

Use `--json` to return an array with each expression, command, description, next due date, timezone, mutex state, repeat interval, environments, and single-server flag. An empty schedule returns `[]`.

```sh
./artisan schedule:list --json --next --timezone=UTC
```

The terminal list shows relative due times, repeat intervals, and whether a task holds a mutex. Use `-v` for the absolute timestamp with its timezone offset and the task description. For Artisan tasks, verbose mode shows the running binary followed by `artisan`. Pass `--no-ansi` to remove terminal colors. JSON output is always plain.

```sh
./artisan schedule:list -v --no-ansi
```

Sub-minute tasks keep `schedule:run` active through the end of the current minute. `./artisan schedule:interrupt` signals repeat loops in that minute through the selected cache store. Task failures go to the exception reporter and remaining due tasks still run. The command exits successfully after processing task failures. Scheduler infrastructure errors exit with a failure status.

Use `./artisan schedule:test --name=reports:daily` to run a selected task immediately in the foreground. With an interactive terminal, omit `--name` to select from the registered tasks. Use `./artisan schedule:clear-cache` to remove overlap locks for registered tasks, including locks in the shared Redis store.

### Testing scheduled tasks {#testing-scheduled-tasks}

The scheduler uses the `TestApp` clock to decide when tasks are due and to compute the next times shown by `schedule:list`. An explicit `Schedule::useClock` takes precedence. In an active repeat minute, `schedule:list` shows the next seconds boundary. Sub-minute timers still measure elapsed time. See [Testing time](../testing/mocking.md#interacting-with-time).

```rust
use openrust::{support::facades::Schedule, testing::TestApp};

#[tokio::test]
async fn runs_a_task_with_a_frozen_clock() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| {
        builder.withCommands(|_| Schedule::configure(|schedule| {
            schedule.doNotAllowPausing();
            schedule.call(|_, output| Box::pin(async move {
                output.stdout.push_str("scheduled task ran\n");
                Ok(())
            })).everyMinute();
            Ok(())
        }))
    })?;

    app.freezeSecond().await?;
    app.artisan(["schedule:run"]).await
        .assertSuccessful()
        .expectsOutput("scheduled task ran");
    Ok(())
}
```

## Task output {#task-output}

Use `sendOutputTo` to replace a file or `appendOutputTo` to append to it. Both capture stdout and stderr from scheduled Artisan commands and executables, including commands that exit unsuccessfully. Relative paths are resolved from the application root.

```rust
schedule.command("reports:daily", []).sendOutputTo("storage/logs/daily.log");
schedule.command("reports:audit", []).appendOutputTo("storage/logs/audit.log");
```

Use `emailOutputTo(["ops@example.com"])` to send nonempty output after a run. `emailOutputOnFailure(["ops@example.com"])` sends only on a failed run, including failures without output. Both use the application mailer.

## Task hooks {#task-hooks}

Attach a hook to a task or a schedule group. In this example, `hook` creates an asynchronous callback that receives the application and console output.

```rust
schedule
    .command("fixture:hello", [])
    .name("hook-success")
    .before(hook("task-before"))
    .then(hook("task-then"));
```

The scheduler runs `before` hooks before the task. `after` and `then` are aliases. `onSuccess` and `onFailure` run when the task completes with the corresponding result. After hooks run in registration order. Nested groups inherit the outer hooks and add their own. A failing hook is reported through the application exception handler.

Use `thenWithOutput`, `onSuccessWithOutput`, or `onFailureWithOutput` for a hook that receives captured bytes as its third argument. Pass `true` as the second argument to skip empty output. An explicit output file supplies its full contents, including earlier runs in append mode.

### Pinging URLs {#pinging-urls}

Use `pingBefore`, `thenPing`, `pingOnSuccess`, and `pingOnFailure` to send HTTP GET requests at the corresponding hook. Their `If` variants take a boolean followed by the URL. Failed requests are reported and the scheduler continues.

## Background tasks {#background-tasks}

Call `runInBackground()` on a scheduled command or executable. OpenRust runs the task and its after hooks in a separate instance of the application binary. The parent runs the before hooks and can exit while the task continues. Use a shared cache store such as file or Redis with `withoutOverlapping()`.

`ScheduledBackgroundTaskFinished` is dispatched by the child after completion and lock release. It carries the task and exit code, including on failure. Register listeners for `console.scheduled_background_task_finished` during application assembly.

On Windows ARM64, the background child uses the same application executable. File and Redis caches both support releasing the overlap lock after success, a nonzero command exit, or a failed after hook. When the parent exits, callers capturing its stdout and stderr can read EOF while the background task is still running.

## Events {#events}

Import the scheduling event types from `openrust::console::events`. Register a listener in the application assembly scope. This example assumes a `ScheduleEvents` listener and a `console_error` adapter that converts registration errors to `ConsoleError`.

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

A foreground task emits `ScheduledTaskStarting` before execution. A completed command emits `ScheduledTaskFinished`, followed by `ScheduledTaskFailed` if the exit status is nonzero. An execution error emits `ScheduledTaskFailed` after the starting event. The finished and failed events include the elapsed `Duration`. The failed event also carries the exception.

A task skipped by a filter, pause, server election, or overlap lock emits `ScheduledTaskSkipped`.
