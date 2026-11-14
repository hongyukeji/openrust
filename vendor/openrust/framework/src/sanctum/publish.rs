use std::io;

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, generator_command::write_generated,
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

const PROVIDER: &str = "OpenRust\\Sanctum\\SanctumServiceProvider";
const RESOURCES: [(&str, &str, &[u8]); 2] = [
    (
        "sanctum-config",
        "config/sanctum.rs",
        include_bytes!("stubs/sanctum.stub"),
    ),
    (
        "sanctum-migrations",
        "database/migrations/2019_12_14_000001_create_personal_access_tokens_table.rs",
        include_bytes!("stubs/personal_access_tokens.stub"),
    ),
];

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Publish(CommandDefinition::new(
        Syntax::new("vendor:publish")
            .about("Publish configuration and migrations supplied by installed packages")
            .arg(Arg::new("provider").long("provider"))
            .arg(Arg::new("tag").long("tag").action(ArgAction::Append))
            .arg(Arg::new("all").long("all").action(ArgAction::SetTrue))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
            .arg(
                Arg::new("existing")
                    .long("existing")
                    .action(ArgAction::SetTrue),
            ),
    ))))
}

struct Publish(CommandDefinition);

impl Command for Publish {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let options = input.matches();
            let provider = options.get_one::<String>("provider");
            let tags: Vec<_> = options
                .get_many::<String>("tag")
                .into_iter()
                .flatten()
                .collect();
            if provider.is_some_and(|value| value != PROVIDER)
                || tags
                    .iter()
                    .any(|tag| !RESOURCES.iter().any(|(known, _, _)| tag.as_str() == *known))
                || (provider.is_none() && tags.is_empty() && !options.get_flag("all"))
            {
                output.info("No publishable resources for the given selection.");
                return Ok(0);
            }
            let existing = options.get_flag("existing");
            let force = options.get_flag("force");
            for (tag, relative, bytes) in RESOURCES {
                if !tags.is_empty() && !tags.iter().any(|value| value.as_str() == tag) {
                    continue;
                }
                let path = app.base_path().join(relative);
                let written = tokio::task::spawn_blocking(move || {
                    if existing && !path.try_exists()? {
                        return Ok(false);
                    }
                    match write_generated(&path, bytes, force || existing) {
                        Ok(()) => Ok(true),
                        Err(error)
                            if !force
                                && !existing
                                && error.kind() == io::ErrorKind::AlreadyExists =>
                        {
                            Ok(false)
                        }
                        Err(error) => Err(error),
                    }
                })
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{relative}: {error}")))?;
                if written {
                    output.info(&format!("Published {relative}."));
                }
            }
            Ok(0)
        })
    }
}
