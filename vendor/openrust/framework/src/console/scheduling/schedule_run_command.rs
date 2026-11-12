use std::{
    fmt::Write as _,
    io::{self, Write},
    sync::Arc,
    time::{Duration, SystemTime},
};

use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use clap::{Arg, ArgAction};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::{
    cache::{Repository, Ttl, Value},
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    container::EnvironmentResult,
    contracts::cache::Repository as _,
    contracts::foundation::Application as _,
    foundation::Application,
    queue::DispatchOptions,
    support::facades::{Cache, Queue},
};

use super::{
    CacheEventMutex, CacheSchedulingMutex, Event, EventMutex, Schedule, SchedulingMutex, error,
    event::Action,
};

#[derive(Clone, Copy)]
enum Operation {
    Run,
    List,
    Interrupt,
    Work,
}

/// Runs events registered during Artisan bootstrap.
pub struct ScheduleRunCommand {
    definition: CommandDefinition,
    schedule: Arc<Schedule>,
    artisan: Arc<Artisan>,
    operation: Operation,
}

pub(crate) fn register(artisan: &mut Artisan, schedule: Schedule) -> Result<(), ConsoleError> {
    let commands = Arc::new(artisan.clone());
    let schedule = Arc::new(schedule);
    for (operation, name, description) in [
        (
            Operation::Run,
            "schedule:run",
            "Run the scheduled tasks that are due",
        ),
        (Operation::List, "schedule:list", "Show scheduled tasks"),
        (
            Operation::Interrupt,
            "schedule:interrupt",
            "Interrupt running scheduler loops",
        ),
        (
            Operation::Work,
            "schedule:work",
            "Run the scheduler continuously",
        ),
    ] {
        let mut syntax = clap::Command::new(name).about(description);
        if matches!(operation, Operation::Run) {
            syntax = syntax.arg(
                Arg::new("whisper")
                    .long("whisper")
                    .action(ArgAction::SetTrue)
                    .help("Suppress the message when no tasks run"),
            );
        }
        artisan.add_command(Box::new(ScheduleRunCommand {
            definition: CommandDefinition::new(syntax),
            schedule: schedule.clone(),
            artisan: commands.clone(),
            operation,
        }))?;
    }
    Ok(())
}

impl Command for ScheduleRunCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            match self.operation {
                Operation::List => self.list(app, output)?,
                Operation::Interrupt => self.interrupt(output).await?,
                Operation::Run | Operation::Work => {
                    let shutdown = CancellationToken::new();
                    let signals =
                        crate::console::monitor_signals(shutdown.clone()).map_err(error)?;
                    let result = if matches!(self.operation, Operation::Run) {
                        self.run(app, output, input.matches().get_flag("whisper"), &shutdown)
                            .await
                    } else {
                        self.work(app, output, &shutdown).await
                    };
                    signals.abort();
                    result?;
                }
            }
            Ok(0)
        })
    }
}

struct Due<'a> {
    event: &'a Event,
    election: Option<bool>,
    next_run: Instant,
}

impl ScheduleRunCommand {
    fn store(&self) -> Result<Arc<Repository>, ConsoleError> {
        Cache::store(self.schedule.store.as_deref()).map_err(error)
    }

    fn list(&self, app: &Application, output: &mut Output) -> Result<(), ConsoleError> {
        let default_zone = timezone(app)?;
        let now = DateTime::<Utc>::from((self.schedule.clock)());
        for event in self.schedule.events() {
            let local = now.with_timezone(&event.zone.unwrap_or(default_zone));
            let next = event
                .expression
                .find_next_occurrence(&local, false)
                .map_err(error)?;
            writeln!(
                output.stdout,
                "{}  {}  Next: {}",
                event.cron_text,
                event.label(),
                next.to_rfc3339()
            )
            .expect("writing to a String");
        }
        if self.schedule.events().next().is_none() {
            output.info("No scheduled tasks have been defined.");
        }
        Ok(())
    }

    async fn interrupt(&self, output: &mut Output) -> Result<(), ConsoleError> {
        let now = (self.schedule.clock)();
        self.store()?
            .put(
                &interrupt_key(now),
                &Value::from(serde_json::json!(true)),
                Some(Ttl::Seconds(60)),
            )
            .await
            .map_err(error)?;
        output.info("Broadcasting schedule interruption signal.");
        Ok(())
    }

    async fn work(
        &self,
        app: &Application,
        output: &mut Output,
        shutdown: &CancellationToken,
    ) -> Result<(), ConsoleError> {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                _ = tokio::time::sleep(remaining_in_minute((self.schedule.clock)())) => {},
            }
            self.run(app, output, false, shutdown).await?;
            io::stdout()
                .write_all(std::mem::take(&mut output.stdout).as_bytes())
                .map_err(error)?;
            io::stderr()
                .write_all(std::mem::take(&mut output.stderr).as_bytes())
                .map_err(error)?;
        }
    }

    async fn run(
        &self,
        app: &Application,
        output: &mut Output,
        whisper: bool,
        shutdown: &CancellationToken,
    ) -> Result<(), ConsoleError> {
        let start = Instant::now();
        let time = (self.schedule.clock)();
        let deadline = start + remaining_in_minute(time);
        let default_zone = timezone(app)?;
        let EnvironmentResult::String(environment) = app.environment(None).map_err(error)? else {
            return Err(ConsoleError::new(
                "Could not resolve the application's environment.",
            ));
        };
        let mut due = Vec::new();
        for event in self.schedule.events() {
            if !event.runsInMaintenanceMode() && app.isDownForMaintenance().await.map_err(error)? {
                continue;
            }
            if event.due(time.into(), default_zone)? {
                due.push(Due {
                    event,
                    election: None,
                    next_run: start,
                });
            }
        }
        let repeats = due.iter().any(|entry| entry.event.interval.is_some());
        let store = if repeats
            || due
                .iter()
                .any(|entry| entry.event.one_server || entry.event.overlap.is_some())
        {
            Some(self.store()?)
        } else {
            None
        };
        let overlap = store.clone().map(CacheEventMutex::new);
        let election = store.clone().map(CacheSchedulingMutex::new);
        let mut ran = false;

        for entry in &mut due {
            if shutdown.is_cancelled() {
                break;
            }
            ran |= self
                .run_event(
                    app,
                    output,
                    &environment,
                    entry,
                    time,
                    overlap.as_ref(),
                    election.as_ref(),
                )
                .await?;
        }
        if !ran && !whisper {
            output.info("No scheduled commands are ready to run.");
        }
        let Some(store) = store.filter(|_| repeats) else {
            return Ok(());
        };
        let interrupted = interrupt_key(time);
        let mut has_entered_maintenance = false;
        loop {
            let wake = due
                .iter()
                .filter(|entry| entry.event.interval.is_some() && entry.election != Some(false))
                .map(|entry| entry.next_run)
                .min()
                .unwrap_or(deadline)
                .min(deadline)
                .min(Instant::now() + Duration::from_millis(100));
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                _ = sleep_until(wake) => {},
            }
            if Instant::now() >= deadline || store.get(&interrupted).await.map_err(error)?.is_some()
            {
                return Ok(());
            }
            for entry in &mut due {
                if shutdown.is_cancelled() || Instant::now() >= deadline {
                    return Ok(());
                }
                if let Some(interval) = entry.event.interval
                    && entry.election != Some(false)
                    && entry.next_run <= Instant::now()
                {
                    has_entered_maintenance = has_entered_maintenance
                        || app.isDownForMaintenance().await.map_err(error)?;
                    if has_entered_maintenance && !entry.event.runsInMaintenanceMode() {
                        entry.next_run = Instant::now() + interval;
                        continue;
                    }
                    self.run_event(
                        app,
                        output,
                        &environment,
                        entry,
                        time,
                        overlap.as_ref(),
                        election.as_ref(),
                    )
                    .await?;
                }
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Keep the run's cache handles and task state explicit"
    )]
    async fn run_event(
        &self,
        app: &Application,
        output: &mut Output,
        environment: &str,
        entry: &mut Due<'_>,
        time: SystemTime,
        overlap: Option<&CacheEventMutex>,
        election: Option<&CacheSchedulingMutex>,
    ) -> Result<bool, ConsoleError> {
        let event = entry.event;
        if let Some(interval) = event.interval {
            entry.next_run = Instant::now() + interval;
        }
        if !event.allowed(app, environment) {
            return Ok(false);
        }
        if let Some(election) = election.filter(|_| event.one_server) {
            if entry.election.is_none() {
                entry.election = Some(election.create(event, time).await.map_err(error)?);
            }
            if entry.election == Some(false) {
                return Ok(false);
            }
        }
        let overlap = overlap.filter(|_| event.overlap.is_some());
        if let Some(overlap) = overlap
            && !overlap.create(event).await.map_err(error)?
        {
            return Ok(false);
        }
        let result = self.invoke(app, output, event).await;
        let release = match overlap {
            Some(overlap) => overlap.forget(event).await.map_err(error),
            None => Ok(()),
        };
        if let Err(error) = result {
            writeln!(
                output.stderr,
                "Scheduled task '{}' failed: {error}",
                event.label()
            )
            .expect("writing to a String");
            let _ = app.report(&error).await;
        }
        release?;
        Ok(true)
    }

    async fn invoke(
        &self,
        app: &Application,
        output: &mut Output,
        event: &Event,
    ) -> Result<(), ConsoleError> {
        match &event.action {
            Action::Callback(callback) => callback(app, output).await,
            Action::Command(arguments) => {
                let status = self.artisan.handle(app, arguments, output).await;
                if status == 0 {
                    Ok(())
                } else {
                    Err(error(format!("Artisan exited with status {status}")))
                }
            }
            Action::Exec(arguments) => {
                let result = tokio::process::Command::new(&arguments[0])
                    .args(&arguments[1..])
                    .current_dir(app.base_path())
                    .kill_on_drop(true)
                    .output()
                    .await
                    .map_err(error)?;
                output
                    .stdout
                    .push_str(&String::from_utf8_lossy(&result.stdout));
                output
                    .stderr
                    .push_str(&String::from_utf8_lossy(&result.stderr));
                if result.status.success() {
                    Ok(())
                } else {
                    Err(error(result.status))
                }
            }
            Action::Job(payload, options) => {
                Queue::dispatch(
                    payload.for_scheduled_dispatch().map_err(error)?,
                    DispatchOptions {
                        connection: options.connection.clone(),
                        queue: options.queue.clone(),
                        ..DispatchOptions::default()
                    },
                    None,
                )
                .await
                .map_err(error)?;
                Ok(())
            }
        }
    }
}

fn timezone(app: &Application) -> Result<Tz, ConsoleError> {
    let config = app.configuration_repository().map_err(error)?;
    let zone = config
        .get("app.schedule_timezone")
        .filter(|value| !value.is_null())
        .or_else(|| config.get("app.timezone").filter(|value| !value.is_null()));
    match zone.as_ref() {
        None => Ok(chrono_tz::UTC),
        Some(zone) => zone
            .as_str()
            .ok_or_else(|| error("The schedule timezone must be a string."))?
            .parse()
            .map_err(error),
    }
}

fn remaining_in_minute(now: SystemTime) -> Duration {
    let utc = DateTime::<Utc>::from(now);
    Duration::from_secs(u64::from(60 - utc.second()))
        - Duration::from_nanos(u64::from(utc.nanosecond()))
}

fn interrupt_key(now: SystemTime) -> String {
    let slot = DateTime::<Utc>::from(now).timestamp().div_euclid(60);
    format!("schedule:interrupt:{slot}")
}
