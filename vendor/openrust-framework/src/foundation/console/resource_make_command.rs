use std::{borrow::Cow, io::ErrorKind, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        GeneratorCommand, Input, Output,
        generator_command::{Stub, snake_case, source_name_parts, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let mut syntax = Syntax::new("make:resource")
        .about("Create a new resource")
        .arg(
            Arg::new("name")
                .required(true)
                .help("The name of the resource"),
        );
    for (option, short, help) in [
        ("force", 'f', "Replace an existing resource"),
        ("json-api", 'j', "Create a JSON:API resource"),
        ("collection", 'c', "Create a resource collection"),
    ] {
        syntax = syntax.arg(
            Arg::new(option)
                .long(option)
                .short(short)
                .help(help)
                .action(ArgAction::SetTrue),
        );
    }
    artisan.add_command(Box::new(ResourceMakeCommand(CommandDefinition::new(
        syntax,
    ))))
}

struct ResourceMakeCommand(CommandDefinition);

impl Command for ResourceMakeCommand {
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
            let path = self.get_path(app, name(input), input)?;
            let label = match stub(input) {
                Stub::ResourceCollection => "Resource collection",
                _ => "Resource",
            };
            let relative = path
                .strip_prefix(app.base_path())
                .unwrap_or(&path)
                .to_owned();
            let force = input.matches().get_flag("force");
            let template = self.get_stub(app, input)?;
            let result = tokio::task::spawn_blocking(move || {
                let module = path.with_extension("").join("mod.rs");
                if module.try_exists()? {
                    return Err(std::io::Error::other(format!(
                        "{} already defines the module",
                        module.display()
                    )));
                }
                write_generated(&path, template.as_bytes(), force)
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))?;
            match result {
                Ok(()) => {}
                // resource command reports the conflict while exiting successfully.
                Err(error) if !force && error.kind() == ErrorKind::AlreadyExists => {
                    output
                        .stderr
                        .push_str(&format!("{label} already exists.\n"));
                    return Ok(0);
                }
                Err(error) => {
                    return Err(ConsoleError::new(format!(
                        "{}: {error}",
                        relative.display()
                    )));
                }
            }
            output.info(&format!(
                "{label} [{}] created successfully.",
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the resource module in its parent mod.rs and rebuild.\n");
            Ok(0)
        })
    }
}

impl GeneratorCommand for ResourceMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        let (directory, name) = resource_path(name);
        let mut path = app.base_path().join(directory);
        path.extend(parts(name)?.map(snake_case));
        Ok(path.with_extension("rs"))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let (_, name) = resource_path(name(input));
        let class = parts(name)?.last().expect("validated resource name");
        let template = stub(input)
            .load(app.base_path())
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        Ok(Cow::Owned(template.replace("{{class}}", class)))
    }
}

fn name(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("name required by Clap")
}

fn resource_path(name: &str) -> (&'static str, &str) {
    let name = name.trim_start_matches(['/', '\\']);
    match name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"))
    {
        Some(name) => ("app", name),
        None => ("app/http/resources", name),
    }
}

fn parts(name: &str) -> Result<impl Iterator<Item = &str>, ConsoleError> {
    source_name_parts(
        name,
        "Use a PascalCase resource name, such as UserResource.",
    )
}

fn stub(input: &Input) -> Stub {
    let name = name(input);
    if input.matches().get_flag("collection")
        || name
            .strip_suffix(".rs")
            .unwrap_or(name)
            .ends_with("Collection")
    {
        Stub::ResourceCollection
    } else if input.matches().get_flag("json-api") {
        Stub::JsonApiResource
    } else {
        Stub::Resource
    }
}
