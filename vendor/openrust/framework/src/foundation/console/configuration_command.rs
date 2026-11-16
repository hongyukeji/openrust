use std::{ffi::OsString, fmt::Write as _, path::PathBuf};

use clap::Arg;

use crate::{
    config::{Value, cache},
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, command_position,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, application::ConfigurationCacheMode},
};

#[derive(Clone, Copy)]
enum Operation {
    Cache,
    Clear,
    Show,
}

struct ConfigurationCommand {
    definition: CommandDefinition,
    operation: Operation,
}

impl Command for ConfigurationCommand {
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
            match self.operation {
                Operation::Cache => {
                    let path = app
                        .container()
                        .get::<PathBuf>("config.cache.path")
                        .map_err(error)?;
                    cache::save(&path, app.config().map_err(error)?.as_ref()).map_err(error)?;
                    output.info("Configuration cached successfully.");
                }
                Operation::Clear => {
                    let path = app
                        .container()
                        .get::<PathBuf>("config.cache.path")
                        .map_err(error)?;
                    cache::clear(&path).map_err(error)?;
                    output.info("Configuration cache cleared successfully.");
                }
                Operation::Show => {
                    let key = input
                        .matches()
                        .get_one::<String>("config")
                        .expect("required configuration key");
                    let value = app.config().map_err(error)?.get(key).ok_or_else(|| {
                        ConsoleError::new(format!(
                            "Configuration file or key {key} does not exist."
                        ))
                    })?;
                    output.stdout.push('\n');
                    if matches!(value, Value::Array(_) | Value::Object(_)) {
                        let _ = writeln!(output.stdout, "  {key}");
                        render(&mut output.stdout, "", &value);
                    } else {
                        render(&mut output.stdout, key, &value);
                    }
                    output.stdout.push('\n');
                }
            }
            Ok(0)
        })
    }
}

fn render(output: &mut String, key: &str, value: &Value) {
    let child = |name: &str| {
        if key.is_empty() {
            name.to_owned()
        } else {
            format!("{key}.{name}")
        }
    };
    match value {
        Value::Object(items) if !items.is_empty() => {
            for (name, value) in items {
                render(output, &child(name), value);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                render(output, &child(&index.to_string()), value);
            }
        }
        Value::Object(_) | Value::Array(_) if key.is_empty() => {}
        Value::Object(_) | Value::Array(_) => {
            let _ = writeln!(output, "  {key}  []");
        }
        Value::String(value) if !value.chars().any(char::is_control) => {
            let _ = writeln!(output, "  {key}  {value}");
        }
        _ => {
            let _ = writeln!(output, "  {key}  {value}");
        }
    }
}

pub(super) fn cache_mode(arguments: &[OsString]) -> ConfigurationCacheMode {
    let Some(name) = command_position(arguments).and_then(|position| arguments[position].to_str())
    else {
        return ConfigurationCacheMode::Use;
    };
    if arguments
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        return ConfigurationCacheMode::Use;
    }
    let Some((namespace, suffix)) = name.split_once(':') else {
        return ConfigurationCacheMode::Use;
    };
    if namespace.is_empty() || !"config".starts_with(namespace) || suffix.is_empty() {
        return ConfigurationCacheMode::Use;
    }
    match ("cache".starts_with(suffix), "clear".starts_with(suffix)) {
        (true, false) => ConfigurationCacheMode::Refresh,
        (false, true) => ConfigurationCacheMode::Clear,
        _ => ConfigurationCacheMode::Use,
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (operation, name, about) in [
        (
            Operation::Cache,
            "config:cache",
            "Create a cache file for faster configuration loading",
        ),
        (
            Operation::Clear,
            "config:clear",
            "Remove the configuration cache file",
        ),
        (
            Operation::Show,
            "config:show",
            "Display all of the values for a given configuration file or key",
        ),
    ] {
        let mut syntax = clap::Command::new(name).about(about);
        if matches!(operation, Operation::Show) {
            syntax = syntax.arg(
                Arg::new("config")
                    .required(true)
                    .help("The configuration file or key to show"),
            );
        }
        artisan.add_command(Box::new(ConfigurationCommand {
            definition: CommandDefinition::new(syntax),
            operation,
        }))?;
    }
    Ok(())
}

fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
