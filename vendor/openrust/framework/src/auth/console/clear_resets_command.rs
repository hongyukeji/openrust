use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::Application,
    support::facades::Password,
};

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ClearResetsCommand(CommandDefinition::new(
        clap::Command::new("auth:clear-resets")
            .about("Remove expired password reset tokens")
            .arg(clap::Arg::new("name")),
    ))))
}

struct ClearResetsCommand(CommandDefinition);

impl Command for ClearResetsCommand {
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
                .get_one::<String>("name")
                .map(String::as_str);
            let broker = Password::broker(name).map_err(console_error)?;
            broker
                .getRepository()
                .deleteExpired()
                .await
                .map_err(console_error)?;
            output.info("Expired password reset tokens cleared.");
            Ok(0)
        })
    }
}

fn console_error(error: crate::auth::AuthError) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
