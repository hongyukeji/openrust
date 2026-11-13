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
            .timezone(chrono_tz::Asia::Shanghai)
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
    .timezone(chrono_tz::Asia::Tokyo)
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

Sub-minute tasks keep `schedule:run` active through the end of the current minute. `./artisan schedule:interrupt` signals repeat loops in that minute through the selected cache store. Task failures go to the exception reporter and remaining due tasks still run. The command exits successfully after processing task failures. Scheduler infrastructure errors exit with a failure status.

Use `./artisan schedule:test --name=reports:daily` to run a selected task immediately in the foreground. With an interactive terminal, omit `--name` to select from the registered tasks. Use `./artisan schedule:clear-cache` to remove overlap locks for registered tasks, including locks in the shared Redis store.

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
