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
    foundation::Application,
};

use super::source_edit::file_error;

use crate::translation::bundled::CATALOGS;

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
            let path = app.langPath("");
            let existing = input.matches().get_flag("existing");
            let force = input.matches().get_flag("force");
            tokio::task::spawn_blocking(move || {
                for (name, contents) in CATALOGS {
                    let target = path.join(name);
                    let published = target
                        .try_exists()
                        .map_err(|error| file_error(&target, error))?;
                    if existing && !published || !existing && !force && published {
                        continue;
                    }
                    let directory = target
                        .parent()
                        .ok_or_else(|| ConsoleError::new("语言资源路径无效"))?;
                    fs::create_dir_all(directory).map_err(|error| file_error(directory, error))?;
                    let mut temporary = tempfile::NamedTempFile::new_in(directory)
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
