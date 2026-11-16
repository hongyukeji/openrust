use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::Stdio,
};

use crate::{
    config::cache::ConfigurationCache,
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

use super::Kernel;

pub(in crate::foundation) const WRITE_COMMAND: &str = "__openrust_config_write";

/// Marks applications constructed by the executable entry point.
pub(in crate::foundation) struct ExecutableEntry;

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (name, description, cache) in [
        (
            "config:cache",
            "Create a cache file for faster configuration loading",
            true,
        ),
        (
            "config:clear",
            "Remove the cached configuration file",
            false,
        ),
    ] {
        artisan.add_command(Box::new(ConfigCacheCommand {
            definition: CommandDefinition::new(clap::Command::new(name).about(description)),
            cache,
        }))?;
    }
    Ok(())
}

struct ConfigCacheCommand {
    definition: CommandDefinition,
    cache: bool,
}

impl Command for ConfigCacheCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let path = app
                .container()
                .get::<PathBuf>("config.cache.path")
                .map_err(error)?;
            if self.cache {
                let selected = input
                    .matches()
                    .get_many::<String>("env")
                    .and_then(|mut args| args.next());
                refresh(app, &path, selected.map(String::as_str)).await?;
                output.info("Configuration cached successfully.");
            } else {
                ConfigurationCache::clear(&path).map_err(error)?;
                output.info("Configuration cache cleared successfully.");
            }
            Ok(0)
        })
    }
}

async fn refresh(
    app: &Application,
    path: &Path,
    selected: Option<&str>,
) -> Result<(), ConsoleError> {
    if !app.container().bound::<ExecutableEntry>() {
        return Err(ConsoleError::new(
            "Configuration caching requires the application executable entry point.",
        ));
    }
    let executable = std::env::current_exe().map_err(error)?;
    let parent = path
        .parent()
        .ok_or_else(|| ConsoleError::new("Invalid configuration cache path."))?;
    fs::create_dir_all(parent).map_err(error)?;
    let staging = tempfile::Builder::new()
        .prefix(".config-cache-")
        .tempdir_in(parent)
        .map_err(error)?;
    let snapshot = staging.path().join("config.json");
    ConfigurationCache::clear(path).map_err(error)?;

    // The same binary reruns the application's compiled bootstrap and provider constructors.
    let mut command = tokio::process::Command::new(executable);
    command
        .args(["artisan", WRITE_COMMAND])
        .arg(&snapshot)
        .env("APP_BASE_PATH", app.base_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if let Some(selected) = selected {
        command.arg(selected);
    }
    if !command.status().await.map_err(error)?.success() {
        return Err(ConsoleError::new(
            "Unable to build the fresh configuration cache.",
        ));
    }
    if ConfigurationCache::read(&snapshot)
        .map_err(error)?
        .is_none()
    {
        return Err(ConsoleError::new(
            "Fresh configuration cache was not written.",
        ));
    }
    fs::rename(&snapshot, path).map_err(error)
}

pub(in crate::foundation) async fn write_fresh(
    mut app: Application,
    destination: &Path,
    selected: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.boot_without_config_cache(selected).await?;
    let mut kernel = Kernel::for_artisan(app.share());
    kernel.bootstrap_with_environment(selected).await?;
    let config = app.config()?.all();
    let parent = destination
        .parent()
        .ok_or_else(|| ConsoleError::new("Invalid configuration cache staging path."))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, &config)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist_noclobber(destination)?;
    Ok(())
}

fn error(source: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(source.to_string())
}
