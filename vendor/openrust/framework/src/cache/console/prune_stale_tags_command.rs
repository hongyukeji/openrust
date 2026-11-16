use clap::Arg;

use crate::{
    cache::Cache,
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::cache::Repository as _,
    foundation::Application,
};

struct PruneStaleTagsCommand(CommandDefinition);

impl Command for PruneStaleTagsCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let name = input
                .matches()
                .get_one::<String>("store")
                .map(String::as_str);
            let store = Cache::store(name).map_err(super::console_error)?;
            store
                .get_store()
                .flush_stale_tags()
                .await
                .map_err(super::console_error)?;
            output.info("Stale cache tags pruned successfully.");
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let definition = clap::Command::new("cache:prune-stale-tags")
        .about("Prune stale cache tags from the cache (Redis only)")
        .arg(Arg::new("store"));
    artisan.add_command(Box::new(PruneStaleTagsCommand(CommandDefinition::new(
        definition,
    ))))
}
