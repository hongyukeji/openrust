use std::fmt::Write as _;

use clap::Arg;

use crate::{
    config::Value,
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

struct ConfigurationShowCommand(CommandDefinition);

impl Command for ConfigurationShowCommand {
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
            let key = input
                .matches()
                .get_one::<String>("config")
                .expect("required configuration key");
            let value = app
                .config()
                .map_err(|source| ConsoleError::new(source.to_string()))?
                .get(key)
                .ok_or_else(|| {
                    ConsoleError::new(format!("Configuration file or key {key} does not exist."))
                })?;
            output.stdout.push('\n');
            if matches!(value, Value::Array(_) | Value::Object(_)) {
                writeln!(output.stdout, "  {key}").expect("writing to String");
                render(&mut output.stdout, "", &value);
            } else {
                render(&mut output.stdout, key, &value);
            }
            output.stdout.push('\n');
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

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ConfigurationShowCommand(CommandDefinition::new(
        clap::Command::new("config:show")
            .about("Display all of the values for a given configuration file or key")
            .arg(
                Arg::new("config")
                    .required(true)
                    .help("The configuration file or key to show"),
            ),
    ))))
}
