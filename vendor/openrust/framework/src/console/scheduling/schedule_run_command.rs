use std::{
    error::Error,
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime},
};

use clap::{Arg, ArgAction};
use jiff::{SignedDuration, Timestamp, Zoned, tz::TimeZone};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::{
    cache::{Repository, Ttl, Value},
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        events::{
            SchedulePaused, ScheduleResumed, ScheduledBackgroundTaskFinished, ScheduledTaskFailed,
            ScheduledTaskFinished, ScheduledTaskSkipped, ScheduledTaskStarting,
        },
    },
    container::EnvironmentResult,
    contracts::cache::Repository as _,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    queue::DispatchOptions,
    support::facades::{Cache, Event as Events, Queue},
};

use super::{
    CacheEventMutex, CacheSchedulingMutex, Event, EventMutex, Schedule, SchedulingMutex,
    cron_expression_timezone_converter, error,
    event::Action,
    schedule_list_output::{ListedEvent, display_json, display_text},
};

const PAUSED: &str = "illuminate:schedule:paused";

enum TaskOutcome {
    Succeeded,
    NonZeroExit(i32, ConsoleError),
}

#[derive(Clone, Copy)]
enum Operation {
    Run,
    List,
    Interrupt,
    Work,
    Pause,
    Resume,
    Background,
    ClearCache,
    Test,
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
        (Operation::List, "schedule:list", "List all scheduled tasks"),
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
        (Operation::Pause, "schedule:pause", "Pause the scheduler"),
        (Operation::Resume, "schedule:resume", "Resume the schedule"),
        (
            Operation::ClearCache,
            "schedule:clear-cache",
            "Remove cached scheduler locks",
        ),
        (Operation::Test, "schedule:test", "Run a scheduled command"),
        (
            Operation::Background,
            "schedule:finish",
            "Complete a background scheduled task",
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
        if matches!(operation, Operation::List) {
            syntax = syntax
                .arg(
                    Arg::new("environment")
                        .long("environment")
                        .action(ArgAction::Append)
                        .help("Display tasks scheduled for these environments"),
                )
                .arg(
                    Arg::new("timezone")
                        .long("timezone")
                        .help("The timezone that times should be displayed in"),
                )
                .arg(
                    Arg::new("next")
                        .long("next")
                        .action(ArgAction::SetTrue)
                        .help("Sort the listed tasks by their next due date"),
                )
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .help("Output the scheduled tasks as JSON"),
                );
        }
        if matches!(operation, Operation::Background) {
            syntax = syntax
                .hide(true)
                .arg(
                    Arg::new("index")
                        .required(true)
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(Arg::new("key").required(true))
                .arg(Arg::new("owner").long("owner"));
        }
        if matches!(operation, Operation::Test) {
            syntax = syntax.arg(
                Arg::new("name")
                    .long("name")
                    .value_name("NAME")
                    .help("The name of the scheduled command to run"),
            );
        }
        if matches!(operation, Operation::Resume) {
            syntax = syntax.visible_alias("schedule:continue");
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
                Operation::List => self.list(app, input, output).await?,
                Operation::ClearCache => self.clear_cache(output).await?,
                Operation::Test => self.test_task(app, input, output).await?,
                Operation::Interrupt => self.interrupt(output).await?,
                Operation::Pause => {
                    if !self.schedule.pausable {
                        output
                            .stderr
                            .push_str("Schedule pausing is currently disabled.\n");
                        return Ok(1);
                    }
                    if !Cache::forever(PAUSED, &Value::from(serde_json::json!(true)))
                        .await
                        .map_err(error)?
                    {
                        return Err(ConsoleError::new("Could not persist the scheduler pause."));
                    }
                    Events::dispatch(SchedulePaused).await.map_err(error)?;
                    output.info("Scheduled task processing has been paused.");
                }
                Operation::Resume => {
                    Cache::forget(PAUSED).await.map_err(error)?;
                    Events::dispatch(ScheduleResumed).await.map_err(error)?;
                    output.info("Scheduled task processing has resumed.");
                }
                Operation::Background => return self.finish_background(app, input, output).await,
                Operation::Run | Operation::Work => {
                    if let Some(shutdown) = input.shutdown() {
                        self.work(app, output, shutdown).await?;
                        return Ok(0);
                    }
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
    index: usize,
    event: &'a Arc<Event>,
    election: Option<bool>,
    next_run: Instant,
}

impl ScheduleRunCommand {
    fn store(&self) -> Result<Arc<Repository>, ConsoleError> {
        Cache::store(self.schedule.store.as_deref()).map_err(error)
    }

    async fn list(
        &self,
        app: &Application,
        input: &Input,
        output: &mut Output,
    ) -> Result<(), ConsoleError> {
        let json = input.matches().get_flag("json");
        let environments = input.matches().get_many::<String>("environment");
        let events: Vec<_> = self
            .schedule
            .events()
            .filter(|event| {
                environments
                    .clone()
                    .is_none_or(|mut names| names.any(|name| event.runs_in_environment(name)))
            })
            .collect();
        if events.is_empty() {
            if json {
                output.stdout.push_str("[]\n");
            } else {
                output.info("No scheduled tasks have been defined.");
            }
            return Ok(());
        }

        let display_zone = match input.matches().get_one::<String>("timezone") {
            Some(name) => crate::support::Date::timezone(name).map_err(error)?,
            None => parse_timezone(
                app.configuration_repository()
                    .map_err(error)?
                    .get("app.timezone"),
            )?,
        };
        let default_zone = timezone(app)?;
        let now = Timestamp::try_from(self.schedule.current_time()?).map_err(error)?;
        let mut rows = Vec::new();
        for event in events {
            let local = now.to_zoned(event.zone.clone().unwrap_or_else(|| default_zone.clone()));
            let cron_next = event
                .expression
                .find_next_occurrence(&local, false)
                .map_err(error)?;
            let next = if let Some(interval) = event.interval
                && event.due(now, default_zone.clone())?
            {
                // Repeats advance to the next interval boundary after the current second.
                let seconds = interval.as_secs();
                let offset = seconds - local.second() as u64 % seconds;
                local
                    .with()
                    .subsec_nanosecond(0)
                    .build()
                    .and_then(|time| time.checked_add(SignedDuration::from_secs(offset as i64)))
                    .map_err(error)?
            } else {
                cron_next.clone()
            };
            rows.push((next.with_time_zone(display_zone.clone()), cron_next, event));
        }
        if input.matches().get_flag("next") {
            rows.sort_by_key(|(next, _, _)| next.timestamp());
        }
        let verbose = input.matches().get_count("verbose") > 0;
        let executable = if verbose
            && rows
                .iter()
                .any(|(_, _, event)| matches!(event.action, Action::Command(_)))
        {
            Some(std::env::current_exe().map_err(error)?)
        } else {
            None
        };
        let mutex = CacheEventMutex::new(self.store()?);
        let displayed_now = now.to_zoned(display_zone.clone());
        let mut listed = Vec::with_capacity(rows.len());
        for (next, cron_next, event) in rows {
            let expressions = cron_expression_timezone_converter::for_event(
                &event.cron_text,
                cron_next,
                display_zone.clone(),
            );
            let (command, arguments_start) = match &event.action {
                Action::Command(args) => {
                    let prefix = executable.as_ref().map_or_else(
                        || "./artisan ".to_owned(),
                        |path| format!("{} artisan ", path.display()),
                    );
                    let arguments_start = (args.len() > 1)
                        .then(|| prefix.len() + args[0].to_string_lossy().len() + 1);
                    (prefix + &event.command_name(), arguments_start)
                }
                Action::Exec(_) => (event.command_name(), None),
                Action::Callback(_, at) if event.description.is_none() => {
                    let source = Path::new(at.file());
                    let source = source.strip_prefix(app.base_path()).unwrap_or(source);
                    (
                        format!("Closure at: {}:{}", source.display(), at.line()),
                        None,
                    )
                }
                _ => (event.getSummaryForDisplay(), None),
            };
            listed.push(ListedEvent {
                event,
                expressions,
                command,
                arguments_start,
                next_due_date: next.strftime("%Y-%m-%d %H:%M:%S %:z").to_string(),
                next_due_date_human: relative_due(&displayed_now, &next),
                timezone: display_zone.iana_name().unwrap_or("UTC"),
                has_mutex: mutex.exists(event).await.map_err(error)?,
            });
        }
        if json {
            display_json(&listed, output);
        } else {
            display_text(&listed, verbose, output);
        }
        Ok(())
    }

    async fn clear_cache(&self, output: &mut Output) -> Result<(), ConsoleError> {
        let mutex = CacheEventMutex::new(self.store()?);
        let mut cleared = false;
        for task in self.schedule.events() {
            if mutex.exists(task).await.map_err(error)? {
                mutex.clear(task).await.map_err(error)?;
                output.info(&format!("Deleting mutex for [{}]", task.command_name()));
                cleared = true;
            }
        }
        if !cleared {
            output.info("No mutex files were found.");
        }
        Ok(())
    }

    async fn test_task(
        &self,
        app: &Application,
        input: &Input,
        output: &mut Output,
    ) -> Result<(), ConsoleError> {
        let tasks: Vec<_> = self.schedule.events().collect();
        if tasks.is_empty() {
            output.info("No scheduled commands have been defined.");
            return Ok(());
        }
        let task = if let Some(name) = input
            .matches()
            .get_one::<String>("name")
            .filter(|name| !name.is_empty())
        {
            let mut matches = tasks
                .iter()
                .copied()
                .filter(|task| task.command_name() == *name);
            matches.next().filter(|_| matches.next().is_none())
        } else {
            if !io::stdin().is_terminal() {
                return Err(error(
                    "Use --name to select a scheduled task without an interactive terminal.",
                ));
            }
            let mut terminal = io::stderr().lock();
            writeln!(terminal, "Which command would you like to run?").map_err(error)?;
            for (index, task) in tasks.iter().enumerate() {
                writeln!(terminal, "  {}. {}", index + 1, task.command_name()).map_err(error)?;
            }
            terminal.flush().map_err(error)?;
            drop(terminal);
            let mut choice = String::new();
            io::stdin().read_line(&mut choice).map_err(error)?;
            choice
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| tasks.get(index).copied())
        };
        let Some(task) = task else {
            output.info("No matching scheduled command found.");
            return Ok(());
        };
        output.info(&format!(
            "Running [{}]{}",
            task.command_name(),
            if task.background {
                " normally in background"
            } else {
                ""
            }
        ));
        let mutex = task
            .overlap
            .map(|_| self.store().map(CacheEventMutex::new))
            .transpose()?;
        if let Some(mutex) = &mutex
            && !mutex.create(task).await.map_err(error)?
        {
            return Ok(());
        }
        let result = self.invoke_with_hooks(app, output, task).await;
        if let Some(mutex) = &mutex {
            mutex.forget(task).await.map_err(error)?;
        }
        result?;
        Ok(())
    }

    async fn is_paused(&self) -> Result<bool, ConsoleError> {
        if !self.schedule.pausable {
            return Ok(false);
        }
        Ok(Cache::get(PAUSED).await.map_err(error)? == Some(Value::from(serde_json::json!(true))))
    }

    async fn interrupt(&self, output: &mut Output) -> Result<(), ConsoleError> {
        let now = self.schedule.current_time()?;
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
            let time = self.schedule.current_time()?;
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                _ = tokio::time::sleep(remaining_in_minute(time)) => {},
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
        let time = self.schedule.current_time()?;
        let deadline = start + remaining_in_minute(time);
        let default_zone = timezone(app)?;
        let EnvironmentResult::String(environment) = app.environment(None).map_err(error)? else {
            return Err(ConsoleError::new(
                "Could not resolve the application's environment.",
            ));
        };
        let mut due = Vec::new();
        for (index, event) in self.schedule.event_handles().enumerate() {
            if !event.runsInMaintenanceMode()
                && app.is_down_for_maintenance().await.map_err(error)?
            {
                continue;
            }
            if event.due(
                Timestamp::try_from(time).map_err(error)?,
                default_zone.clone(),
            )? && event.runs_in_environment(&environment)
            {
                due.push(Due {
                    index,
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
        let mut paused = !due.is_empty() && self.is_paused().await?;

        for entry in &mut due {
            if shutdown.is_cancelled() {
                break;
            }
            if paused && !entry.event.runsWhenPaused() {
                dispatch_skipped(entry.event).await?;
                continue;
            }
            ran |= self
                .run_event(
                    app,
                    output,
                    entry,
                    time,
                    overlap.as_ref(),
                    election.as_ref(),
                )
                .await?;
        }
        if let Some(store) = store.filter(|_| repeats) {
            let interrupted = interrupt_key(time);
            let mut has_entered_maintenance = false;
            'repeat: loop {
                let wake = due
                    .iter()
                    .filter(|entry| {
                        entry.event.interval.is_some()
                            && entry.election != Some(false)
                            && (!paused || entry.event.runsWhenPaused())
                    })
                    .map(|entry| entry.next_run)
                    .min()
                    .unwrap_or(deadline)
                    .min(deadline)
                    .min(Instant::now() + Duration::from_millis(100));
                tokio::select! {
                    _ = shutdown.cancelled() => break 'repeat,
                    _ = sleep_until(wake) => {},
                }
                if Instant::now() >= deadline
                    || store.get(&interrupted).await.map_err(error)?.is_some()
                {
                    break 'repeat;
                }
                paused = self.is_paused().await?;
                for entry in &mut due {
                    if shutdown.is_cancelled() || Instant::now() >= deadline {
                        break 'repeat;
                    }
                    if let Some(interval) = entry.event.interval
                        && entry.election != Some(false)
                        && entry.next_run <= Instant::now()
                    {
                        has_entered_maintenance = has_entered_maintenance
                            || app.is_down_for_maintenance().await.map_err(error)?;
                        if has_entered_maintenance && !entry.event.runsInMaintenanceMode() {
                            entry.next_run = Instant::now() + interval;
                            continue;
                        }
                        if paused && !entry.event.runsWhenPaused() {
                            dispatch_skipped(entry.event).await?;
                            continue;
                        }
                        ran |= self
                            .run_event(
                                app,
                                output,
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
        if !ran && !whisper {
            output.info("No scheduled commands are ready to run.");
        }
        Ok(())
    }

    async fn run_event(
        &self,
        app: &Application,
        output: &mut Output,
        entry: &mut Due<'_>,
        time: SystemTime,
        overlap: Option<&CacheEventMutex>,
        election: Option<&CacheSchedulingMutex>,
    ) -> Result<bool, ConsoleError> {
        let event = entry.event;
        if let Some(interval) = event.interval {
            entry.next_run = Instant::now() + interval;
        }
        if !event.filters_pass(app) {
            dispatch_skipped(event).await?;
            return Ok(false);
        }
        if let Some(election) = election.filter(|_| event.one_server) {
            if entry.election.is_none() {
                entry.election = Some(election.create(event, time).await.map_err(error)?);
            }
            if entry.election == Some(false) {
                dispatch_skipped(event).await?;
                return Ok(false);
            }
        }
        let overlap = overlap.filter(|_| event.overlap.is_some());
        if let Some(overlap) = overlap
            && !overlap.create(event).await.map_err(error)?
        {
            dispatch_skipped(event).await?;
            return Ok(false);
        }
        let execution = async {
            Events::dispatch(ScheduledTaskStarting {
                task: event.clone(),
            })
            .await
            .map_err(error)?;
            let start = Instant::now();
            let result = if event.background {
                self.launch_background(app, output, entry.index, event, overlap)
                    .await
            } else {
                self.invoke_with_hooks(app, output, event).await
            };
            Ok::<_, ConsoleError>((result, start.elapsed()))
        }
        .await;
        let release = match overlap {
            Some(overlap) => overlap.forget(event).await.map_err(error),
            None => Ok(()),
        };
        let (result, runtime) = execution?;
        let failure: Option<Arc<dyn Error + Send + Sync>> = match result {
            Ok(outcome) => Events::dispatch(ScheduledTaskFinished {
                task: event.clone(),
                runtime,
            })
            .await
            .err()
            .map(|exception| Arc::new(exception) as Arc<dyn Error + Send + Sync>)
            .or_else(|| match outcome {
                TaskOutcome::Succeeded => None,
                TaskOutcome::NonZeroExit(_, exception) => Some(Arc::new(exception)),
            }),
            Err(exception) => Some(Arc::new(exception)),
        };
        if let Some(error) = failure {
            Events::dispatch(ScheduledTaskFailed {
                task: event.clone(),
                exception: error.clone(),
                runtime: Some(runtime),
            })
            .await
            .map_err(super::error)?;
            writeln!(
                output.stderr,
                "Scheduled task '{}' failed: {error}",
                event.getSummaryForDisplay()
            )
            .expect("writing to a String");
            let _ = app.report(error.as_ref()).await;
        }
        release?;
        Ok(true)
    }

    async fn invoke_with_hooks(
        &self,
        app: &Application,
        output: &mut Output,
        event: &Event,
    ) -> Result<TaskOutcome, ConsoleError> {
        event.call_before_hooks(app, output).await?;
        self.invoke_with_after_hooks(app, output, event).await
    }

    async fn invoke_with_after_hooks(
        &self,
        app: &Application,
        output: &mut Output,
        event: &Event,
    ) -> Result<TaskOutcome, ConsoleError> {
        let (outcome, captured) = self.invoke(app, output, event).await?;
        if outcome.is_ok() || matches!(event.action, Action::Callback(..) | Action::Job(..)) {
            event
                .call_after_hooks(
                    app,
                    output,
                    matches!(outcome, Ok(TaskOutcome::Succeeded)),
                    &captured,
                )
                .await?;
        }
        outcome
    }

    async fn invoke(
        &self,
        app: &Application,
        output: &mut Output,
        event: &Event,
    ) -> Result<(Result<TaskOutcome, ConsoleError>, Vec<u8>), ConsoleError> {
        let command = matches!(event.action, Action::Command(_) | Action::Exec(_));
        if !command && (event.destination.is_some() || event.captures_output() || event.background)
        {
            return Err(error(
                "Output and background options require a scheduled command or executable.",
            ));
        }
        let mut destination = event
            .destination
            .as_ref()
            .map(|destination| destination.open(app))
            .transpose()?;
        let capture = event.captures_output();
        let mut captured = Vec::new();
        let outcome = match &event.action {
            Action::Callback(callback, _) => {
                callback(app, output).await.map(|()| TaskOutcome::Succeeded)
            }
            Action::Command(arguments) => {
                let mut command_output = Output::default();
                let target = if destination.is_some() || capture {
                    &mut command_output
                } else {
                    output
                };
                let status = self.artisan.handle(app, arguments, target).await;
                if let Some(file) = &mut destination {
                    file.write_all(command_output.stdout.as_bytes())
                        .map_err(error)?;
                    file.write_all(command_output.stderr.as_bytes())
                        .map_err(error)?;
                }
                if capture {
                    captured = command_output.stdout.into_bytes();
                    captured.extend(command_output.stderr.into_bytes());
                }
                Ok(exit_outcome(
                    status,
                    format!("Artisan exited with status {status}"),
                ))
            }
            Action::Exec(arguments) => {
                let mut command = tokio::process::Command::new(&arguments[0]);
                command
                    .args(&arguments[1..])
                    .current_dir(app.base_path())
                    .kill_on_drop(true);
                if destination.is_some() || capture {
                    let file = match destination.take() {
                        Some(file) => file,
                        None => tempfile::tempfile().map_err(error)?,
                    };
                    command
                        .stdout(Stdio::from(file.try_clone().map_err(error)?))
                        .stderr(Stdio::from(file.try_clone().map_err(error)?));
                    let status = command.status().await.map_err(error)?;
                    if capture {
                        use std::io::{Read, Seek};
                        if let Some(destination) = &event.destination {
                            captured = tokio::fs::read(app.base_path().join(&destination.path))
                                .await
                                .map_err(error)?;
                        } else {
                            let mut file = file;
                            file.rewind().map_err(error)?;
                            file.read_to_end(&mut captured).map_err(error)?;
                        }
                    }
                    Ok(exit_outcome(status.code().unwrap_or(1), status))
                } else {
                    let result = command.output().await.map_err(error)?;
                    output
                        .stdout
                        .push_str(&String::from_utf8_lossy(&result.stdout));
                    output
                        .stderr
                        .push_str(&String::from_utf8_lossy(&result.stderr));
                    Ok(exit_outcome(
                        result.status.code().unwrap_or(1),
                        result.status,
                    ))
                }
            }
            Action::Job(payload, options) => Queue::dispatch(
                payload.for_scheduled_dispatch().map_err(error)?,
                DispatchOptions {
                    connection: options.connection.clone(),
                    queue: options.queue.clone(),
                    ..DispatchOptions::default()
                },
                None,
            )
            .await
            .map(|_| TaskOutcome::Succeeded)
            .map_err(error),
        };
        if capture
            && matches!(event.action, Action::Command(_))
            && let Some(destination) = &event.destination
        {
            captured = tokio::fs::read(app.base_path().join(&destination.path))
                .await
                .map_err(error)?;
        }
        Ok((outcome, captured))
    }

    async fn launch_background(
        &self,
        app: &Application,
        output: &mut Output,
        index: usize,
        event: &Event,
        overlap: Option<&CacheEventMutex>,
    ) -> Result<TaskOutcome, ConsoleError> {
        if overlap.is_some() {
            let config = app.configuration_repository().map_err(error)?;
            let default = config.get("cache.default");
            let name = self
                .schedule
                .store
                .as_deref()
                .filter(|name| !name.is_empty())
                .or_else(|| default.as_ref().and_then(serde_json::Value::as_str))
                .unwrap_or("database");
            if config
                .get(&format!("cache.stores.{name}.driver"))
                .and_then(|driver| driver.as_str().map(str::to_owned))
                .as_deref()
                == Some("array")
            {
                return Err(error(
                    "Background overlap prevention requires a shared cache store.",
                ));
            }
        }
        if !matches!(event.action, Action::Command(_) | Action::Exec(_)) {
            return Err(error(
                "Background scheduling requires a command or executable.",
            ));
        }
        event.call_before_hooks(app, output).await?;
        let environment_file = app.container().get::<PathBuf>("env.file").map_err(error)?;
        let executable = std::env::current_exe().map_err(error)?;
        let mut args = vec![
            "artisan".into(),
            "schedule:finish".into(),
            index.to_string(),
            event.mutex_key(),
        ];
        if let Some(environment) = environment_file
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(".env."))
        {
            args.push(format!("--env={environment}"));
        }
        let owner = match overlap {
            Some(overlap) => overlap.take_owner(event).await,
            None => None,
        };
        if let Some(owner) = &owner {
            args.extend(["--owner".into(), owner.clone()]);
        }
        match spawn_background(&executable, app.base_path(), &args) {
            Ok(()) => Ok(TaskOutcome::Succeeded),
            Err(source) => {
                if let (Some(overlap), Some(owner)) = (overlap, owner) {
                    overlap.release_owner(event, &owner).await.map_err(error)?;
                }
                Err(error(source))
            }
        }
    }

    async fn finish_background(
        &self,
        app: &Application,
        input: &Input,
        output: &mut Output,
    ) -> Result<i32, ConsoleError> {
        let args = input.matches();
        let index = *args.get_one::<usize>("index").expect("required index");
        let key = args.get_one::<String>("key").expect("required key");
        let owner = args.get_one::<String>("owner");
        let lock = owner
            .map(|owner| {
                use crate::contracts::cache::LockProvider as _;
                self.store()?.restore_lock(key, owner).map_err(error)
            })
            .transpose()?;
        let task = self
            .schedule
            .event_handles()
            .nth(index)
            .filter(|task| task.background && task.mutex_key() == *key);
        let start = Instant::now();
        let result = match task {
            Some(task) => self.invoke_with_after_hooks(app, output, task).await,
            None => Err(error("The scheduled background task could not be found.")),
        };
        let release = match lock {
            Some(mut lock) => lock.release().await.map(|_| ()).map_err(error),
            None => Ok(()),
        };
        let (code, failure) = match result {
            Ok(TaskOutcome::Succeeded) => (0, None),
            Ok(TaskOutcome::NonZeroExit(code, failure)) => (code, Some(failure)),
            Err(failure) => (1, Some(failure)),
        };
        if let Some(task) = task {
            let failed_event = if let Some(failure) = failure {
                let failure = Arc::new(failure);
                let _ = app.report(failure.as_ref()).await;
                Events::dispatch(ScheduledTaskFailed {
                    task: task.clone(),
                    exception: failure,
                    runtime: Some(start.elapsed()),
                })
                .await
                .map(|_| ())
                .map_err(error)
            } else {
                Ok(())
            };
            release?;
            let finished = Events::dispatch(ScheduledBackgroundTaskFinished {
                task: task.clone(),
                exit_code: code,
            })
            .await
            .map_err(error);
            failed_event?;
            finished?;
        } else {
            release?;
            return Err(failure.expect("missing task has a failure"));
        }
        Ok(code)
    }
}

#[cfg(windows)]
fn spawn_background(executable: &Path, base_path: &Path, args: &[String]) -> io::Result<()> {
    crate::process::windows_background::spawn(executable, base_path, args, None)
}

#[cfg(not(windows))]
fn spawn_background(executable: &Path, base_path: &Path, args: &[String]) -> io::Result<()> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .env("APP_BASE_PATH", base_path)
        .current_dir(base_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

fn exit_outcome(code: i32, message: impl std::fmt::Display) -> TaskOutcome {
    if code == 0 {
        TaskOutcome::Succeeded
    } else {
        TaskOutcome::NonZeroExit(code, error(message))
    }
}

fn timezone(app: &Application) -> Result<TimeZone, ConsoleError> {
    let config = app.configuration_repository().map_err(error)?;
    let zone = config
        .get("app.schedule_timezone")
        .filter(|value| !value.is_null())
        .or_else(|| config.get("app.timezone").filter(|value| !value.is_null()));
    parse_timezone(zone)
}

fn relative_due(now: &Zoned, next: &Zoned) -> String {
    let month_number = |date: &Zoned| i64::from(date.year()) * 12 + i64::from(date.month() - 1);
    let mut months = month_number(next) - month_number(now);
    if months > 0 && (next.day(), next.time()) < (now.day(), now.time()) {
        months -= 1;
    }

    let days = next.date().duration_since(now.date()).as_secs() / 86_400
        - i64::from(next.time() < now.time());
    let seconds = next.timestamp().duration_since(now.timestamp()).as_secs();
    let (count, unit) = if months >= 12 {
        (months / 12, "year")
    } else if months > 0 {
        (months, "month")
    } else if days >= 7 {
        (days / 7, "week")
    } else if days > 0 {
        (days, "day")
    } else if seconds >= 3_600 {
        (seconds / 3_600, "hour")
    } else if seconds >= 60 {
        (seconds / 60, "minute")
    } else {
        (seconds.max(0), "second")
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} from now")
}

fn parse_timezone(zone: Option<serde_json::Value>) -> Result<TimeZone, ConsoleError> {
    match zone.as_ref().filter(|value| !value.is_null()) {
        None => Ok(TimeZone::UTC),
        Some(zone) => crate::support::Date::timezone(
            zone.as_str()
                .ok_or_else(|| error("The schedule timezone must be a string."))?,
        )
        .map_err(error),
    }
}

async fn dispatch_skipped(event: &Arc<Event>) -> Result<(), ConsoleError> {
    Events::dispatch(ScheduledTaskSkipped {
        task: event.clone(),
    })
    .await
    .map_err(error)?;
    Ok(())
}

fn remaining_in_minute(now: SystemTime) -> Duration {
    let utc = Timestamp::try_from(now)
        .expect("valid scheduling timestamp")
        .to_zoned(TimeZone::UTC);
    Duration::from_secs((60 - utc.second()) as u64)
        - Duration::from_nanos(utc.subsec_nanosecond() as u64)
}

fn interrupt_key(now: SystemTime) -> String {
    let slot = Timestamp::try_from(now)
        .expect("valid scheduling timestamp")
        .as_nanosecond()
        .div_euclid(60_000_000_000);
    format!("schedule:interrupt:{slot}")
}
