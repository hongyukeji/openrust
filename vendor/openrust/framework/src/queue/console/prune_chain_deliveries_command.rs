use chrono::{TimeDelta, Utc};
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

struct PruneChainDeliveriesCommand(CommandDefinition);

impl Command for PruneChainDeliveriesCommand {
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
            let hours = *args.get_one::<u32>("hours").expect("default hours");
            let cutoff = Utc::now()
                .checked_sub_signed(TimeDelta::hours(i64::from(hours)))
                .ok_or_else(|| {
                    ConsoleError::new("The chain receipt retention period is out of range.")
                })?;
            let queue = app
                .container_handle()
                .make::<QueueManager>()
                .map_err(error)?;
            let connection = args.get_one::<String>("connection").map(String::as_str);
            let deleted = queue
                .prune_chain_deliveries(connection, cutoff.timestamp())
                .await
                .map_err(error)?;
            output.info(&format!("{deleted} chain delivery receipts deleted."));
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:prune-chain-deliveries")
        .about("Prune confirmed chain receipts on a database queue connection")
        .arg(Arg::new("connection").help("The target queue connection"))
        .arg(
            Arg::new("hours")
                .long("hours")
                .default_value("24")
                .value_parser(value_parser!(u32))
                .help("The number of hours to retain confirmed receipts"),
        );
    artisan.add_command(Box::new(PruneChainDeliveriesCommand(
        CommandDefinition::new(syntax),
    )))
}
