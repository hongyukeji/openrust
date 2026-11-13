use std::{
    error::Error,
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::Stdio,
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
    CacheEventMutex, CacheSchedulingMutex, Event, EventMutex, Schedule, SchedulingMutex, error,
    event::Action,
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
                Operation::List => self.list(app, output)?,
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
                event.getSummaryForDisplay(),
                next.to_rfc3339()
            )
            .expect("writing to a String");
        }
        if self.schedule.events().next().is_none() {
            output.info("No scheduled tasks have been defined.");
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
        for (index, event) in self.schedule.event_handles().enumerate() {
            if !event.runsInMaintenanceMode() && app.isDownForMaintenance().await.map_err(error)? {
                continue;
            }
            if event.due(time.into(), default_zone)? && event.runs_in_environment(&environment) {
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
                            || app.isDownForMaintenance().await.map_err(error)?;
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
                runtime,
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
        if outcome.is_ok() || matches!(event.action, Action::Callback(_) | Action::Job(..)) {
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
            Action::Callback(callback) => {
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
        let mut command = tokio::process::Command::new(std::env::current_exe().map_err(error)?);
        command
            .args([
                "artisan",
                "schedule:finish",
                &index.to_string(),
                &event.mutex_key(),
            ])
            .env("APP_BASE_PATH", app.base_path())
            .current_dir(app.base_path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(environment) = environment_file
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(".env."))
        {
            command.arg(format!("--env={environment}"));
        }
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
        let owner = match overlap {
            Some(overlap) => overlap.take_owner(event).await,
            None => None,
        };
        if let Some(owner) = &owner {
            command.args(["--owner", owner]);
        }
        match command.spawn() {
            Ok(mut child) => {
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                Ok(TaskOutcome::Succeeded)
            }
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
                    runtime: start.elapsed(),
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

fn exit_outcome(code: i32, message: impl std::fmt::Display) -> TaskOutcome {
    if code == 0 {
        TaskOutcome::Succeeded
    } else {
        TaskOutcome::NonZeroExit(code, error(message))
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

async fn dispatch_skipped(event: &Arc<Event>) -> Result<(), ConsoleError> {
    Events::dispatch(ScheduledTaskSkipped {
        task: event.clone(),
    })
    .await
    .map_err(error)?;
    Ok(())
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
