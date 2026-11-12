use std::time::Duration;

use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    queue::{
        QueueManager, Worker, WorkerOptions,
        worker::WorkerCadence,
        worker_options::MEBIBYTE,
        worker_signals::{WorkerSignalState, WorkerSignals},
    },
};

use super::worker_output::WorkerOutput;

/// Dispatches the queue:work command to a worker.
pub struct WorkCommand(CommandDefinition);

impl WorkCommand {
    pub(super) fn new() -> Self {
        Self(CommandDefinition::new(
            Syntax::new("queue:work")
                .about("Run a queue worker")
                .arg(Arg::new("connection"))
                .arg(Arg::new("queue").long("queue"))
                .arg(flag("once"))
                .arg(flag("json").help("Output the job information as JSON"))
                .arg(flag("stop-when-empty"))
                .arg(seconds("stop-when-empty-for").default_value("0"))
                .arg(seconds("rest").default_value("0"))
                .arg(seconds("tries").default_value("1"))
                .arg(seconds("timeout").default_value("60"))
                .arg(seconds("sleep").default_value("3"))
                .arg(seconds("backoff").value_delimiter(',').num_args(1))
                .arg(
                    Arg::new("max-jobs")
                        .long("max-jobs")
                        .value_parser(value_parser!(u64)),
                )
                .arg(seconds("max-time"))
                .arg(Arg::new("memory").long("memory").default_value("128").allow_hyphen_values(true)
                    .help("Resident memory limit in MiB or as a percentage of the configured worker memory budget")),
        ))
    }
}

impl Command for WorkCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let args = input.matches();
            let container = app.container_handle();
            let manager = container.make::<QueueManager>().map_err(error)?;
            let connection = manager
                .connection(args.get_one::<String>("connection").map(String::as_str))
                .map_err(error)?;
            let worker = Worker::new(container).map_err(error)?;
            let queues = args
                .get_one::<String>("queue")
                .map(|queues| queues.split(',').collect::<Vec<_>>())
                .unwrap_or_default();
            let once = args.get_flag("once");
            let number = |name| *args.get_one::<u32>(name).expect("Clap default");
            let config = app.config().map_err(error)?;
            let memory = args.get_one::<String>("memory").expect("Clap default");
            let budget = config
                .get("queue.memory_limit")
                .and_then(|value| value.as_u64());
            let options = WorkerOptions {
                tries: number("tries"),
                timeout: duration(number("timeout")),
                sleep: duration(number("sleep")),
                backoff: args
                    .get_many::<u32>("backoff")
                    .into_iter()
                    .flatten()
                    .copied()
                    .map(duration)
                    .collect(),
                max_jobs: if once {
                    Some(1)
                } else {
                    args.get_one::<u64>("max-jobs")
                        .copied()
                        .filter(|&limit| limit > 0)
                },
                max_time: args
                    .get_one::<u32>("max-time")
                    .copied()
                    .filter(|&limit| limit > 0)
                    .map(duration),
                stop_when_empty: once || args.get_flag("stop-when-empty"),
                memory: memory_limit(memory, budget)?,
                stop_on_lost_connection: config
                    .get("queue.stop_on_lost_connection")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(true),
            };
            let mut cadence = if once {
                WorkerCadence {
                    single_run: true,
                    ..WorkerCadence::default()
                }
            } else {
                let empty = number("stop-when-empty-for");
                WorkerCadence {
                    single_run: false,
                    rest: duration(number("rest")),
                    stop_when_empty_for: (empty > 0).then(|| duration(empty)),
                    signal_state: None,
                }
            };
            let timezone = config
                .get("queue.output_timezone")
                .and_then(|value| value.as_str().map(str::to_owned))
                .or_else(|| {
                    config
                        .get("app.timezone")
                        .and_then(|value| value.as_str().map(str::to_owned))
                })
                .map(|zone| zone.parse().map_err(error))
                .transpose()?
                .unwrap_or(chrono_tz::UTC);
            let mut status = WorkerOutput::new(output, args, timezone);
            let shutdown = CancellationToken::new();
            let result = if once {
                let listener = crate::console::monitor_signals(shutdown.clone()).map_err(error)?;
                let result = worker
                    .daemon_with_status(
                        &*connection,
                        &queues,
                        &options,
                        &cadence,
                        &shutdown,
                        |event| status.write(event),
                    )
                    .await;
                listener.abort();
                result
            } else {
                let (signal_state, state) = watch::channel(WorkerSignalState::Running);
                cadence.signal_state = Some(state);
                let mut signals = WorkerSignals::new(
                    connection.get_connection_name(),
                    if queues.is_empty() {
                        connection.default_queue().to_owned()
                    } else {
                        queues.join(",")
                    },
                    &options,
                )
                .map_err(error)?;
                let run = worker.daemon_with_status(
                    &*connection,
                    &queues,
                    &options,
                    &cadence,
                    &shutdown,
                    |event| status.write(event),
                );
                tokio::pin!(run);
                tokio::select! {
                    biased;
                    () = signals.monitor(&worker, &manager, &shutdown, &signal_state) => run.await,
                    result = &mut run => result,
                }
            };
            result.map(|(_, exit_code)| exit_code).map_err(error)
        })
    }
}

fn memory_limit(value: &str, budget: Option<u64>) -> Result<u64, ConsoleError> {
    let invalid =
        || ConsoleError::new("--memory requires a whole number of MiB or a finite percentage");
    let Some(percent) = value.strip_suffix('%') else {
        let mib = value.parse::<i64>().map_err(|_| invalid())?;
        return u64::try_from(mib)
            .unwrap_or_default()
            .checked_mul(MEBIBYTE)
            .ok_or_else(invalid);
    };
    let percent = percent
        .parse::<f64>()
        .ok()
        .filter(|percent| percent.is_finite())
        .ok_or_else(invalid)?;
    if percent <= 0.0 {
        return Ok(0);
    }
    let bytes = budget
        .filter(|budget| *budget > 0)
        .and_then(|budget| budget.checked_mul(MEBIBYTE))
        .ok_or_else(|| {
            ConsoleError::new(
                "a positive queue.memory_limit in MiB is required for --memory percentages",
            )
        })?;
    let limit = ((percent / 100.0) * bytes as f64).ceil().max(1.0);
    if limit >= u64::MAX as f64 {
        return Err(invalid());
    }
    Ok(limit as u64)
}

fn flag(name: &'static str) -> Arg {
    Arg::new(name).long(name).action(ArgAction::SetTrue)
}
fn seconds(name: &'static str) -> Arg {
    Arg::new(name).long(name).value_parser(value_parser!(u32))
}
fn duration(seconds: u32) -> Duration {
    Duration::from_secs(u64::from(seconds))
}

pub(super) fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
