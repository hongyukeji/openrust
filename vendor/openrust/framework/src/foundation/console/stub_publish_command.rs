use std::{io, path::Path};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        generator_command::{Stub, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::{Application, events::PublishingStubs},
    support::facades::Event,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(StubPublishCommand(CommandDefinition::new(
        Syntax::new("stub:publish")
            .about("Publish source templates available for customization")
            .arg(
                Arg::new("existing")
                    .long("existing")
                    .help("Replace only previously published stubs")
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("force")
                    .long("force")
                    .help("Replace any existing stubs")
                    .action(ArgAction::SetTrue),
            ),
    ))))
}

struct StubPublishCommand(CommandDefinition);

impl Command for StubPublishCommand {
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
            let directory = app.base_path().join("stubs");
            tokio::fs::create_dir_all(&directory)
                .await
                .map_err(|error| ConsoleError::new(format!("{}: {error}", directory.display())))?;
            let event = PublishingStubs::default();
            for (name, source) in Stub::ALL.map(Stub::file) {
                event
                    .insert(name, source)
                    .expect("valid built-in stub filename");
            }
            Event::dispatch(event.clone())
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let existing = input.matches().get_flag("existing");
            let force = input.matches().get_flag("force");
            tokio::task::spawn_blocking(move || {
                for (name, source) in event.snapshot() {
                    let path = directory.join(name);
                    publish(&path, source.as_bytes(), existing, force).map_err(|error| {
                        io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                    })?;
                }
                Ok::<_, io::Error>(())
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))?
            .map_err(|error| ConsoleError::new(error.to_string()))?;
            output.info("Stubs published successfully.");
            Ok(0)
        })
    }
}

fn publish(path: &Path, bytes: &[u8], existing: bool, force: bool) -> io::Result<()> {
    if existing && !path.try_exists()? {
        return Ok(());
    }
    match write_generated(path, bytes, existing || force) {
        Err(error) if !existing && !force && error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        result => result,
    }
}
