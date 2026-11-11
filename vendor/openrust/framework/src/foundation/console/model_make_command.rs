use std::{borrow::Cow, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        GeneratorCommand, Input, Output,
        generator_command::{snake_case, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ModelMakeCommand(CommandDefinition::new(
        Syntax::new("make:model")
            .about("Create a new Eloquent model")
            .arg(Arg::new("name").required(true))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue)),
    ))))
}

struct ModelMakeCommand(CommandDefinition);

impl Command for ModelMakeCommand {
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
            let path = self.get_path(app, model_name(input), input)?;
            let stub = self.get_stub(app, input)?;
            let force = input.matches().get_flag("force");
            let relative = path
                .strip_prefix(app.base_path())
                .unwrap_or(&path)
                .to_owned();
            tokio::task::spawn_blocking(move || write_generated(&path, stub.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "Model [{}] created successfully.",
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the model module in its parent mod.rs and rebuild.\n");
            Ok(0)
        })
    }
}

impl GeneratorCommand for ModelMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        let mut path = app.base_path().join("app/models");
        for part in validated_parts(name)? {
            path.push(snake_case(part));
        }
        Ok(path.with_extension("rs"))
    }

    fn get_stub(&self, _: &Application, input: &Input) -> Result<Cow<'static, str>, ConsoleError> {
        let parts = validated_parts(model_name(input))?;
        let class = parts.last().expect("validated model name");
        let table = pluralizer::pluralize(&snake_case(class), 2, false);
        Ok(Cow::Owned(
            include_str!("stubs/model.rs.stub")
                .replace("{{model}}", class)
                .replace("{{table}}", &table),
        ))
    }
}

fn model_name(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("name required by Clap")
}

fn validated_parts(name: &str) -> Result<Vec<&str>, ConsoleError> {
    let parts: Vec<_> = name.split(['/', '\\']).collect();
    if parts.iter().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        // These module names cannot be used, even with a raw identifier.
        || matches!(snake_case(part).as_str(), "self" | "super" | "crate" | "mod")
    }) {
        return Err(ConsoleError::new(
            "Use a PascalCase model name, such as Flight.",
        ));
    }
    Ok(parts)
}
