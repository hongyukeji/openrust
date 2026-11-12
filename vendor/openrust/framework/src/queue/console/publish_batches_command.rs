use std::fmt::Write as _;

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

struct PublishBatchesCommand(CommandDefinition);

impl Command for PublishBatchesCommand {
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
            let limit = *input
                .matches()
                .get_one::<u64>("limit")
                .expect("default limit");
            let queue = app
                .container_handle()
                .make::<QueueManager>()
                .map_err(error)?;
            let repository = queue.batch_repository().map_err(error)?;
            let mut published = 0;
            let mut status = 0;
            for id in repository
                .pending_publications(limit)
                .await
                .map_err(error)?
            {
                match queue.publish_batch(&repository, &id).await {
                    Ok(true) => published += 1,
                    Ok(false) => {}
                    Err(error) => {
                        let _ = writeln!(
                            output.stderr,
                            "Could not publish batch [{}]: {error}",
                            id.id()
                        );
                        status = 1;
                    }
                }
            }
            output.info(&format!("{published} Redis batches published."));
            Ok(status)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:publish-batches")
        .about("Publish pending Redis job batches")
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("100")
                .value_parser(value_parser!(u64).range(1..=1000))
                .help("Maximum number of pending batches to attempt"),
        );
    artisan.add_command(Box::new(PublishBatchesCommand(CommandDefinition::new(
        syntax,
    ))))
}
