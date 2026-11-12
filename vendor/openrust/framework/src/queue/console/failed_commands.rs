use std::{collections::HashSet, fmt::Write as _, time::Duration};

use clap::{Arg, ArgAction, Command as Syntax, value_parser};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::container::Container as _,
    foundation::Application,
    queue::{QueueManager, failed::FailedJobProviderInterface},
};

use super::work_command::error;

enum Action {
    List,
    Retry,
    Forget,
    Flush,
    Prune,
}
struct FailedCommand(CommandDefinition, Action);

impl Command for FailedCommand {
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
            let container = app.container_handle();
            let provider = container
                .get::<dyn FailedJobProviderInterface>("queue.failer")
                .map_err(error)?;
            let args = input.matches();
            match self.1 {
                Action::List => {
                    let jobs = provider.all().await.map_err(error)?;
                    if args.get_flag("json") {
                        let data: Vec<_> = jobs
                            .iter()
                            .map(|job| {
                                serde_json::json!({
                                    "id": job.id, "connection": job.connection, "queue": job.queue,
                                    "name": job.payload.name(), "failed_at": job.failed_at,
                                })
                            })
                            .collect();
                        output
                            .stdout
                            .push_str(&serde_json::Value::Array(data).to_string());
                        output.stdout.push('\n');
                    } else {
                        for job in jobs {
                            output.stdout.push_str(&format!(
                                "{}  {}  {}@{}  {}\n",
                                job.failed_at,
                                job.id,
                                job.connection,
                                job.queue,
                                job.payload.name()
                            ));
                        }
                    }
                }
                Action::Retry => {
                    let manager = container.make::<QueueManager>().map_err(error)?;
                    retry_failed(&manager, provider.as_ref(), args, output).await?;
                }
                Action::Forget => {
                    let id = args.get_one::<String>("id").expect("Clap required id");
                    if !provider.forget(id).await.map_err(error)? {
                        return Err(ConsoleError::new(format!(
                            "Failed job {id:?} was not found."
                        )));
                    }
                }
                Action::Flush => {
                    let age = args
                        .get_one::<u32>("hours")
                        .map(|&hours| Duration::from_secs(u64::from(hours) * 3600));
                    provider.flush(age).await.map_err(error)?;
                }
                Action::Prune => {
                    let hours = *args.get_one::<u32>("hours").expect("Clap default hours");
                    let before = chrono::Utc::now()
                        .checked_sub_signed(chrono::TimeDelta::hours(i64::from(hours)))
                        .ok_or_else(|| {
                            ConsoleError::new("The failed job retention period is out of range.")
                        })?;
                    let provider = provider.prunable().ok_or_else(|| {
                        ConsoleError::new(
                            "The configured failed job storage driver does not support pruning.",
                        )
                    })?;
                    let count = provider.prune(before).await.map_err(error)?;
                    output.info(&format!("{count} entries deleted."));
                }
            }
            Ok(0)
        })
    }
}

async fn retry_failed(
    manager: &QueueManager,
    provider: &dyn FailedJobProviderInterface,
    args: &clap::ArgMatches,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    let ids: Vec<_> = args
        .get_many::<String>("id")
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let ranges = args
        .get_many::<String>("range")
        .into_iter()
        .flatten()
        .filter_map(|range| {
            let (start, end) = range.split_once('-')?;
            if !start
                .bytes()
                .chain(end.bytes())
                .all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            Some((start.parse::<u64>().ok()?, end.parse::<u64>().ok()?))
        });
    let selected: Box<dyn Iterator<Item = String> + Send> = if ids == ["all"] {
        Box::new(provider.ids(None).await.map_err(error)?.into_iter())
    } else if let Some(queue) = args.get_one::<String>("queue") {
        let jobs = provider.ids(Some(queue)).await.map_err(error)?;
        if jobs.is_empty() {
            let _ = writeln!(
                output.stderr,
                "Unable to find failed jobs for queue [{queue}]."
            );
        }
        Box::new(jobs.into_iter())
    } else {
        let expanded = ranges.flat_map(|(start, end)| {
            (start.min(end)..=start.max(end))
                .map(move |id| if start <= end { id } else { start - (id - end) }.to_string())
        });
        Box::new(
            ids.into_iter()
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .chain(expanded),
        )
    };
    let mut selected = selected.peekable();
    if selected.peek().is_none() {
        output.info("No retryable jobs found.");
        return Ok(());
    }
    output.info("Pushing failed queue jobs back onto the queue.");
    let mut seen = HashSet::new();
    for id in selected {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(job) = provider.find(&id).await.map_err(error)? else {
            let _ = writeln!(output.stderr, "Unable to find failed job with ID [{id}].");
            continue;
        };
        if manager.retry_failed(provider, &job).await.map_err(error)? {
            output.info(&format!("Queued failed job {}.", job.id));
        }
    }
    Ok(())
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (syntax, action) in [
        (
            Syntax::new("queue:failed")
                .about("List failed jobs")
                .arg(Arg::new("json").long("json").action(ArgAction::SetTrue)),
            Action::List,
        ),
        (
            Syntax::new("queue:retry")
                .about("Retry failed jobs")
                .arg(Arg::new("id").num_args(0..))
                .arg(Arg::new("queue").long("queue").value_name("QUEUE"))
                .arg(
                    Arg::new("range")
                        .long("range")
                        .value_name("START-END")
                        .action(ArgAction::Append),
                ),
            Action::Retry,
        ),
        (
            Syntax::new("queue:forget")
                .about("Delete a failed job")
                .arg(Arg::new("id").required(true)),
            Action::Forget,
        ),
        (
            Syntax::new("queue:flush").about("Delete failed jobs").arg(
                Arg::new("hours")
                    .long("hours")
                    .value_parser(value_parser!(u32)),
            ),
            Action::Flush,
        ),
        (
            Syntax::new("queue:prune-failed")
                .about("Prune stale entries from the failed jobs table")
                .arg(
                    Arg::new("hours")
                        .long("hours")
                        .default_value("24")
                        .value_parser(value_parser!(u32))
                        .help("The number of hours to retain failed jobs data"),
                ),
            Action::Prune,
        ),
    ] {
        artisan.add_command(Box::new(FailedCommand(
            CommandDefinition::new(syntax),
            action,
        )))?;
    }
    Ok(())
}
