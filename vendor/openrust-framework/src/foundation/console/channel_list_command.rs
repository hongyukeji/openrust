use crate::{
    broadcasting::ChannelRegistry,
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

struct ChannelListCommand(CommandDefinition);

impl Command for ChannelListCommand {
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
            if container.bound::<ChannelRegistry>() {
                let registry = container
                    .make::<ChannelRegistry>()
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
                for channel in registry.patterns() {
                    output.stdout.push_str(channel);
                    output.stdout.push('\n');
                }
            }
            if output.stdout.is_empty() {
                output.info("Your application doesn't have any registered broadcast channels.");
            }
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ChannelListCommand(CommandDefinition::new(
        clap::Command::new("channel:list").about("List all registered private broadcast channels"),
    ))))
}
