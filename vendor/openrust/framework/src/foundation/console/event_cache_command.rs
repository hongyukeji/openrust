use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, events::DiscoverEvents},
};

struct EventCacheCommand(CommandDefinition);

impl Command for EventCacheCommand {
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
            let container = app.container();
            let result = if container.has(crate::container::ServiceKey::typed::<DiscoverEvents>()) {
                container
                    .make::<DiscoverEvents>()
                    .map_err(|error| ConsoleError::new(error.to_string()))?
                    .cache(app)
            } else {
                DiscoverEvents::new(&[]).cache(app)
            };
            result.map_err(|error| ConsoleError::new(error.to_string()))?;
            output.info("Events cached successfully.");
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(EventCacheCommand(CommandDefinition::new(
        clap::Command::new("event:cache")
            .about("Discover and cache the application's events and listeners"),
    ))))
}
