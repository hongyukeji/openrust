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

struct CacheCommand {
    definition: CommandDefinition,
    forget: bool,
}

impl Command for CacheCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let input = input.matches();
            let store = Cache::store(input.get_one::<String>("store").map(String::as_str))
                .map_err(console_error)?;
            let cleared = if self.forget {
                let key = input.get_one::<String>("key").expect("required cache key");
                store.forget(key).await
            } else {
                store.flush().await
            }
            .map_err(console_error)?;
            if cleared {
                output.info(if self.forget {
                    "The key has been removed from the cache."
                } else {
                    "Application cache cleared successfully."
                });
                Ok(0)
            } else {
                output
                    .stderr
                    .push_str("The cache entry could not be removed.\n");
                Ok(1)
            }
        })
    }
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (name, about, forget) in [
        ("cache:clear", "Flush the application cache", false),
        ("cache:forget", "Remove an item from the cache", true),
    ] {
        let mut command = clap::Command::new(name).about(about);
        if forget {
            command = command.arg(Arg::new("key").required(true));
        }
        command = command.arg(Arg::new("store"));
        artisan.add_command(Box::new(CacheCommand {
            definition: CommandDefinition::new(command),
            forget,
        }))?;
    }
    Ok(())
}
