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

## Preventing overlaps and selecting one server

`withoutOverlapping()` holds a cache lock during execution and releases it on completion, including task errors. Its default lease is 1,440 minutes. Set a different lease with `withoutOverlappingFor(minutes)`. `onOneServer()` elects one runner for an event in each minute. Give a callback or job a stable, distinct `name` before enabling either mutex.

All runners must use the same shared cache store and namespace for cross-server locking. Use the application's default shared store or select one with `schedule.useCache("redis")`.

## Running the scheduler

Call `./artisan schedule:run` each minute from your process scheduler, or use `./artisan schedule:work` to keep a scheduler process running. The worker finishes the current task on SIGTERM or Ctrl+C. Use `./artisan schedule:list` to see expressions and upcoming run times. `schedule:run --whisper` hides the message when no task runs.

Sub-minute tasks keep `schedule:run` active through the end of the current minute. `./artisan schedule:interrupt` signals repeat loops in that minute through the selected cache store. Task failures go to the exception reporter and remaining due tasks still run. The command exits successfully after processing task failures. Scheduler infrastructure errors exit with a failure status.
