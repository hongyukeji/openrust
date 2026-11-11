use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

struct EnvironmentCommand(CommandDefinition);

impl Command for EnvironmentCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        _input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let name = application
                .container()
                .get::<str>("env")
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output.info(&format!("The application environment is [{name}]."));
            Ok(0)
        })
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(EnvironmentCommand(CommandDefinition::new(
        clap::Command::new("env").about("Display the current framework environment"),
    ))))
}
