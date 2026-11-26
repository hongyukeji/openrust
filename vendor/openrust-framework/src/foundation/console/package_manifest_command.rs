use clap::Command as Syntax;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::{Application, PackageManifest},
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (name, description, clear) in [
        (
            "package:discover",
            "Rebuild the discovered package manifest",
            false,
        ),
        ("clear-compiled", "Remove the cached package manifest", true),
    ] {
        artisan.add_command(Box::new(ManifestCommand {
            definition: CommandDefinition::new(Syntax::new(name).about(description)),
            clear,
        }))?;
    }
    Ok(())
}

struct ManifestCommand {
    definition: CommandDefinition,
    clear: bool,
}

impl Command for ManifestCommand {
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
            let root = app.base_path().to_owned();
            let path = app
                .get_cached_packages_path()
                .map(std::path::Path::to_owned)
                .ok_or_else(|| ConsoleError::new("package cache path is unavailable"))?;
            let manifest = app.package_manifest();
            let clear = self.clear;
            if !clear {
                output.info("Discovering packages");
            }
            tokio::task::spawn_blocking(move || {
                if clear {
                    PackageManifest::clear(&root, &path)
                } else {
                    manifest.rebuild(&root, &path)
                }
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))?
            .map_err(|error| ConsoleError::new(error.to_string()))?;
            if !clear {
                for package in manifest.packages() {
                    output.comment(package.name);
                }
            }
            output.info(if clear {
                "Compiled package manifest cleared."
            } else {
                "Package manifest rebuilt."
            });
            Ok(0)
        })
    }
}
