use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    database::seeder,
    foundation::Application,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let flag = |name: &'static str| Arg::new(name).long(name).action(ArgAction::SetTrue);
    let definition = CommandDefinition::new(
        Syntax::new("db:seed")
            .about("Seed the database with records")
            .arg(Arg::new("seeder").value_name("class"))
            .arg(Arg::new("class").long("class"))
            .arg(Arg::new("database").long("database"))
            .arg(flag("force"))
            .arg(flag("no-interaction").short('n')),
    );
    artisan.add_command(Box::new(SeedCommand(definition)))
}

struct SeedCommand(CommandDefinition);

impl Command for SeedCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let matches = input.matches();
            if !super::super::confirm(application, matches)? {
                output.stdout.push_str(&crate::console::command_cancelled());
                output.stdout.push('\n');
                return Ok(1);
            }
            let name = matches
                .get_one::<String>("seeder")
                .or_else(|| matches.get_one::<String>("class"))
                .map(String::as_str);
            let database = matches.get_one::<String>("database").map(String::as_str);
            seeder::run(application, database, name, output)
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            Ok(0)
        })
    }
}
