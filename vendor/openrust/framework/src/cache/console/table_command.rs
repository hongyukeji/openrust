use std::io::ErrorKind;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    database::migrations::MigrationCreator,
    foundation::Application,
};

struct CacheTableCommand(CommandDefinition);

impl Command for CacheTableCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let directory = application.base_path().join("database/migrations");
            match MigrationCreator.create_from_template(
                "create_cache_table",
                &directory,
                include_str!("stubs/cache.rs.stub"),
            ) {
                Ok(_) => {
                    output.info("Migration created successfully.");
                    output.info("Rebuild the application to include the migration.");
                    Ok(0)
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    Err(ConsoleError::new("Migration already exists."))
                }
                Err(error) => Err(super::console_error(error)),
            }
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = clap::Command::new("make:cache-table")
        .visible_alias("cache:table")
        .about("Create a migration for the cache database table");
    artisan.add_command(Box::new(CacheTableCommand(CommandDefinition::new(syntax))))
}
