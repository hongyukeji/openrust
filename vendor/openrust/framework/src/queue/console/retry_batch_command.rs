use std::{collections::HashSet, fmt::Write as _, time::Duration};

use aws_lc_rs::digest::{SHA256, digest};
use clap::{Arg, Command as Syntax, value_parser};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::container::Container as _,
    foundation::Application,
    queue::{QueueManager, failed::FailedJobProviderInterface},
    support::facades::Cache,
};

use super::work_command::error;

struct RetryBatchCommand(CommandDefinition);

impl Command for RetryBatchCommand {
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
            let mut seen = HashSet::new();
            let ids = input
                .matches()
                .get_many::<String>("id")
                .into_iter()
                .flatten()
                .filter(|id| !id.is_empty() && *id != "0" && seen.insert(id.as_str()))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if ids.is_empty() {
                return Ok(0);
            }

            let Some(&isolated_exit) = input.matches().get_one::<u8>("isolated") else {
                return retry(app, output, ids).await;
            };
            let scope = ids.join(",");
            let mut key = String::from("framework/command-queue:retry-batch-");
            for byte in digest(&SHA256, scope.as_bytes()).as_ref() {
                write!(key, "{byte:02x}").expect("writing to a string");
            }
            let mut lock = Cache::lock(&key, Some(Duration::from_secs(3600))).map_err(error)?;
            if !lock.get().await.map_err(error)? {
                output.comment("The [queue:retry-batch] command is already running.");
                return Ok(i32::from(isolated_exit));
            }
            let result = retry(app, output, ids).await;
            let release = lock.release().await.map_err(error);
            let status = result?;
            release?;
            Ok(status)
        })
    }
}

async fn retry(
    app: &Application,
    output: &mut Output,
    ids: Vec<&str>,
) -> Result<i32, ConsoleError> {
    let container = app.container_handle();
    let queue = container.make::<QueueManager>().map_err(error)?;
    let batches = queue.batch_repository().map_err(error)?;
    let failures = container
        .get::<dyn FailedJobProviderInterface>("queue.failer")
        .map_err(error)?;
    let mut status = 0;
    output.info("Pushing failed batch jobs back onto the queue.");
    for id in ids {
        let Some(batch) = batches.find(id).await.map_err(error)? else {
            let _ = writeln!(output.stderr, "Unable to find a batch with ID [{id}].");
            status = 1;
            continue;
        };
        if batch.failed_job_ids.is_empty() {
            let _ = writeln!(
                output.stderr,
                "The batch with ID [{id}] does not contain any failed jobs."
            );
            status = 1;
            continue;
        }
        output.info(&format!(
            "Pushing failed queue jobs of the batch [{id}] back onto the queue."
        ));
        for job_id in batch.failed_job_ids {
            let retried = match failures.find(&job_id).await.map_err(error)? {
                Some(job) if job.payload.batchId() == Some(id) => queue
                    .retry_failed(failures.as_ref(), &job)
                    .await
                    .map_err(error)?,
                _ => false,
            };
            if retried {
                output.info(&format!("Queued failed job {job_id}."));
            } else {
                let _ = writeln!(
                    output.stderr,
                    "Failed job [{job_id}] is no longer available to retry."
                );
            }
        }
    }
    Ok(status)
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:retry-batch")
        .about("Retry the failed jobs for a batch")
        .arg(
            Arg::new("id")
                .num_args(0..)
                .help("The ID of the batch whose failed jobs should be retried"),
        )
        .arg(
            Arg::new("isolated")
                .long("isolated")
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value("0")
                .value_parser(value_parser!(u8))
                .help("Do not run if another instance with the same batch IDs is running"),
        );
    artisan.add_command(Box::new(RetryBatchCommand(CommandDefinition::new(syntax))))
}
