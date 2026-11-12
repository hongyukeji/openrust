use std::time::Duration;

use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use tokio_util::sync::CancellationToken;

use crate::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    queue::{QueueManager, Worker, WorkerOptions},
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
                .arg(seconds("tries").default_value("1"))
                .arg(seconds("timeout").default_value("60"))
                .arg(seconds("sleep").default_value("3"))
                .arg(seconds("backoff").value_delimiter(',').num_args(1))
                .arg(
                    Arg::new("max-jobs")
                        .long("max-jobs")
                        .value_parser(value_parser!(u64)),
                )
                .arg(seconds("max-time")),
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
            };
            let config = app.config().map_err(error)?;
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
            let listener = crate::console::monitor_signals(shutdown.clone()).map_err(error)?;
            let result = worker
                .daemon_with_status(&*connection, &queues, &options, &shutdown, |event| {
                    status.write(event)
                })
                .await;
            listener.abort();
            result.map_err(error)?;
            Ok(0)
        })
    }
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
