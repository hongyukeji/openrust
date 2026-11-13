use std::{
    fs,
    io::{self, Write as _},
};

use clap::{Arg, ArgAction};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

use super::source_edit::file_error;

const CATALOGS: [(&str, &str); 4] = [
    (
        "auth.json",
        include_str!("../../translation/stubs/auth.json"),
    ),
    (
        "pagination.json",
        include_str!("../../translation/stubs/pagination.json"),
    ),
    (
        "passwords.json",
        include_str!("../../translation/stubs/passwords.json"),
    ),
    (
        "validation.json",
        include_str!("../../translation/stubs/validation.json"),
    ),
];

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = clap::Command::new("lang:publish")
        .about("Publish all language files that are available for customization")
        .arg(
            Arg::new("existing")
                .long("existing")
                .action(ArgAction::SetTrue)
                .help("Publish and overwrite only the files that have already been published"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Overwrite any existing files"),
        );
    artisan.add_command(Box::new(LangPublish(CommandDefinition::new(syntax))))
}

struct LangPublish(CommandDefinition);

impl Command for LangPublish {
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
            let path = crate::translation::language_path(app.base_path()).join("en");
            let existing = input.matches().get_flag("existing");
            let force = input.matches().get_flag("force");
            tokio::task::spawn_blocking(move || {
                fs::create_dir_all(&path).map_err(|error| file_error(&path, error))?;
                for (name, contents) in CATALOGS {
                    let target = path.join(name);
                    let published = target
                        .try_exists()
                        .map_err(|error| file_error(&target, error))?;
                    if existing && !published || !existing && !force && published {
                        continue;
                    }
                    let mut temporary = tempfile::NamedTempFile::new_in(&path)
                        .map_err(|error| file_error(&target, error))?;
                    temporary
                        .write_all(contents.as_bytes())
                        .map_err(|error| file_error(&target, error))?;
                    if published {
                        let permissions = fs::metadata(&target)
                            .map_err(|error| file_error(&target, error))?
                            .permissions();
                        temporary
                            .as_file()
                            .set_permissions(permissions)
                            .map_err(|error| file_error(&target, error))?;
                    }
                    let result = if existing || force {
                        temporary.persist(&target)
                    } else {
                        temporary.persist_noclobber(&target)
                    };
                    if let Err(error) = result
                        && (existing || force || error.error.kind() != io::ErrorKind::AlreadyExists)
                    {
                        return Err(file_error(&target, error.error));
                    }
                }
                Ok::<_, ConsoleError>(())
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))??;
            output.info("Language files published successfully.");
            Ok(0)
        })
    }
}
