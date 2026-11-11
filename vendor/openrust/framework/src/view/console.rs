use std::{fs, io, path::Path};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

use super::{Factory, ViewError, ViewResult, compilers::Compiler, factory::is_blade_path};

struct ViewCacheCommand {
    definition: CommandDefinition,
    precompile: bool,
}

impl Command for ViewCacheCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let views = app
                .container()
                .get::<Factory>(ServiceKey::typed::<Factory>())
                .map_err(console_error)?;
            tokio::task::spawn_blocking({
                let precompile = self.precompile;
                move || {
                    views.compiler().clear()?;
                    if precompile {
                        for path in views.paths() {
                            compile_directory(&views, path)?;
                        }
                    }
                    Ok::<_, ViewError>(())
                }
            })
            .await
            .map_err(console_error)?
            .map_err(console_error)?;
            output.info(if self.precompile {
                "Blade templates cached successfully."
            } else {
                "Compiled views cleared successfully."
            });
            Ok(0)
        })
    }
}

fn compile_directory(views: &Factory, path: &Path) -> ViewResult<()> {
    let files = match fs::read_dir(path) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ViewError::ReadFailed(error)),
    };
    for entry in files {
        let entry = entry.map_err(ViewError::ReadFailed)?;
        let kind = entry.file_type().map_err(ViewError::ReadFailed)?;
        let path = entry.path();
        if kind.is_dir() && entry.file_name() != "vendor" {
            compile_directory(views, &path)?;
        } else if kind.is_file() && is_blade_path(&path) {
            let path = path.canonicalize().map_err(ViewError::ReadFailed)?;
            views.compiler().compile(&path)?;
        }
    }
    Ok(())
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (name, about, precompile) in [
        (
            "view:cache",
            "Compile all of the application's Blade templates",
            true,
        ),
        ("view:clear", "Clear all compiled view files", false),
    ] {
        artisan.add_command(Box::new(ViewCacheCommand {
            definition: CommandDefinition::new(clap::Command::new(name).about(about)),
            precompile,
        }))?;
    }
    Ok(())
}
