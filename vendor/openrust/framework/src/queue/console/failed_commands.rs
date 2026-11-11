use std::time::Duration;

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
                    let ids = args
                        .get_many::<String>("id")
                        .expect("Clap required ids")
                        .collect::<Vec<_>>();
                    let jobs = if ids.iter().any(|id| id.as_str() == "all") {
                        provider.all().await.map_err(error)?
                    } else {
                        let mut jobs = Vec::new();
                        for id in ids {
                            let job = provider.find(id).await.map_err(error)?.ok_or_else(|| {
                                ConsoleError::new(format!("Failed job {id:?} was not found."))
                            })?;
                            jobs.push(job);
                        }
                        jobs
                    };
                    let mut pushed = std::collections::HashSet::new();
                    for job in jobs {
                        if !pushed.insert(job.id.clone()) {
                            continue;
                        }
                        manager
                            .connection(Some(&job.connection))
                            .map_err(error)?
                            .push(
                                &job.payload.clone().renewed().map_err(error)?,
                                Some(&job.queue),
                            )
                            .await
                            .map_err(error)?;
                        provider.forget_retry(&job).await.map_err(error)?;
                        output.info(&format!("Queued failed job {}.", job.id));
                    }
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
            }
            Ok(0)
        })
    }
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
                .arg(Arg::new("id").required(true).num_args(1..)),
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
    ] {
        artisan.add_command(Box::new(FailedCommand(
            CommandDefinition::new(syntax),
            action,
        )))?;
    }
    Ok(())
}
