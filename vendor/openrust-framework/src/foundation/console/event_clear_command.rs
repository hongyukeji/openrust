use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::{Application, events::event_cache},
};

struct EventClearCommand(CommandDefinition);

impl Command for EventClearCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            event_cache::clear(app).map_err(|error| ConsoleError::new(error.to_string()))?;
            output.info("Cached events cleared successfully.");
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(EventClearCommand(CommandDefinition::new(
        clap::Command::new("event:clear").about("Clear all cached events and listeners"),
    ))))
}
