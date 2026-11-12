use chrono::{DateTime, TimeDelta, Utc};
use clap::{Arg, Command as Syntax, value_parser};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::container::Container as _,
    foundation::Application,
    queue::QueueManager,
};

use super::work_command::error;

struct PruneBatchesCommand(CommandDefinition);

impl Command for PruneBatchesCommand {
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
            let now = Utc::now();
            let args = input.matches();
            let hours = cutoff(now, *args.get_one::<u32>("hours").expect("default hours"))?;
            let unfinished = args
                .get_one::<u32>("unfinished")
                .map(|&age| cutoff(now, age))
                .transpose()?;
            let cancelled = args
                .get_one::<u32>("cancelled")
                .map(|&age| cutoff(now, age))
                .transpose()?;
            let batches = app
                .container_handle()
                .make::<QueueManager>()
                .map_err(error)?
                .batch_repository()
                .map_err(error)?;
            let deleted = batches.prune(hours).await.map_err(error)?;
            output.info(&format!("{deleted} entries deleted."));
            if let Some(before) = unfinished {
                let deleted = batches.pruneUnfinished(before).await.map_err(error)?;
                output.info(&format!("{deleted} unfinished entries deleted."));
            }
            if let Some(before) = cancelled {
                let deleted = batches.pruneCancelled(before).await.map_err(error)?;
                output.info(&format!("{deleted} cancelled entries deleted."));
            }
            Ok(0)
        })
    }
}

fn cutoff(now: DateTime<Utc>, hours: u32) -> Result<DateTime<Utc>, ConsoleError> {
    now.checked_sub_signed(TimeDelta::hours(i64::from(hours)))
        .ok_or_else(|| ConsoleError::new("The batch retention period is out of range."))
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:prune-batches")
        .about("Prune stale entries from the batches database")
        .arg(
            Arg::new("hours")
                .long("hours")
                .default_value("24")
                .value_parser(value_parser!(u32))
                .help("The number of hours to retain batch data"),
        )
        .arg(
            Arg::new("unfinished")
                .long("unfinished")
                .value_parser(value_parser!(u32))
                .help("The number of hours to retain unfinished batch data"),
        )
        .arg(
            Arg::new("cancelled")
                .long("cancelled")
                .value_parser(value_parser!(u32))
                .help("The number of hours to retain cancelled batch data"),
        );
    artisan.add_command(Box::new(PruneBatchesCommand(CommandDefinition::new(
        syntax,
    ))))
}
