use std::fs;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::Application,
};

use super::storage_link_command::{command_error, links, remove_symlink};

struct StorageUnlinkCommand(CommandDefinition);

impl Command for StorageUnlinkCommand {
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
            for (link, _) in links(application)? {
                if !fs::symlink_metadata(&link)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                    || !link.exists()
                {
                    continue;
                }
                remove_symlink(&link).map_err(command_error)?;
                output.info(&format!("The [{}] link has been deleted.", link.display()));
            }
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(StorageUnlinkCommand(CommandDefinition::new(
        clap::Command::new("storage:unlink")
            .about("Delete existing symbolic links configured for the application"),
    ))))
}
