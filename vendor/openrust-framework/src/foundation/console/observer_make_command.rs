use std::path::PathBuf;

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        generator_command::{Stub, snake_case, source_name_parts, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

const INVALID_NAME: &str = "Use a PascalCase observer name, such as FlightObserver.";
const INVALID_MODEL: &str = "Use a PascalCase model name, such as Flight or Travel/Flight.";

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ObserverMakeCommand(CommandDefinition::new(
        Syntax::new("make:observer")
            .about("Create a new observer class")
            .arg(Arg::new("name").required(true))
            .arg(
                Arg::new("force")
                    .long("force")
                    .short('f')
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("model")
                    .long("model")
                    .short('m')
                    .help("The model that the observer applies to"),
            ),
    ))))
}

struct ObserverMakeCommand(CommandDefinition);

impl Command for ObserverMakeCommand {
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
            let args = input.matches();
            let name = args
                .get_one::<String>("name")
                .expect("name required by Clap");
            let (mut relative, name) = application_path(name, "app/observers");
            let parts: Vec<_> = source_name_parts(name, INVALID_NAME)?.collect();
            let class = parts.last().expect("validated name");
            relative.extend(parts.iter().map(|part| snake_case(part)));
            let relative = relative.with_extension("rs");
            let path = app.base_path().join(&relative);
            let directory_module = path.with_extension("").join("mod.rs");
            if directory_module.try_exists().map_err(command_error)? {
                return Err(ConsoleError::new(format!(
                    "{} already defines the module.",
                    directory_module.display()
                )));
            }

            let model = args
                .get_one::<String>("model")
                .map(|name| model_path(name))
                .transpose()?;
            let stub = if model.is_some() {
                Stub::Observer
            } else {
                Stub::PlainObserver
            };
            let template = stub.load(app.base_path()).map_err(command_error)?;
            let source = template
                .replace("{{class}}", class)
                .replace("{{model}}", model.as_deref().unwrap_or_default());
            let force = args.get_flag("force");
            tokio::task::spawn_blocking(move || write_generated(&path, source.as_bytes(), force))
                .await
                .map_err(command_error)?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "Observer [{}] created successfully.",
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the observer module before using the generated type.\n");
            Ok(0)
        })
    }
}

fn application_path<'a>(name: &'a str, default: &str) -> (PathBuf, &'a str) {
    match name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"))
    {
        Some(name) => (PathBuf::from("app"), name),
        None => (PathBuf::from(default), name),
    }
}

fn model_path(name: &str) -> Result<String, ConsoleError> {
    let name = name.strip_prefix('\\').unwrap_or(name);
    let (root, name) = application_path(name, "app/models");
    let parts: Vec<_> = source_name_parts(name, INVALID_MODEL)?.collect();
    let class = *parts.last().expect("validated model name");
    let mut path = String::from("crate");
    for part in root.iter() {
        path.push_str("::");
        path.push_str(part.to_str().expect("static module root"));
    }
    for part in parts {
        path.push_str("::r#");
        path.push_str(&snake_case(part));
    }
    path.push_str("::");
    path.push_str(class);
    Ok(path)
}

fn command_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
