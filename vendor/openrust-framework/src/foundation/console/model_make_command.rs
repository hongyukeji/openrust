use std::{borrow::Cow, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        GeneratorCommand, Input, Output,
        generator_command::{Stub, snake_case, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ModelMakeCommand(CommandDefinition::new(
        Syntax::new("make:model")
            .about("Create a new Eloquent model")
            .arg(Arg::new("name").required(true))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
            .arg(
                Arg::new("fields")
                    .long("fields")
                    .action(ArgAction::SetTrue)
                    .help("从数据库表结构自动生成字段引用（扩展）"),
            )
            .arg(Arg::new("table").long("table"))
            .arg(Arg::new("database").long("database").requires("fields")),
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
            let mut stub = self.get_stub(app, input)?.into_owned();
            if input.matches().get_flag("fields") {
                let block = super::model_fields_command::generate(app, input).await?;
                stub = super::model_fields_command::merge(
                    &stub,
                    &block,
                    &validated_parts(model_name(input))?.join("/"),
                )?;
            }
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

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let parts = validated_parts(model_name(input))?;
        let class = parts.last().expect("validated model name");
        let table = super::model_fields_command::table_name(input, class);
        let quoted_table = format!("{table:?}");
        let stub = Stub::Model
            .load(app.base_path())
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        let connection = input.matches().get_one::<String>("database");
        if connection.is_some() && !stub.contains("{{connection}}") {
            return Err(ConsoleError::new(
                "自定义模型模板缺少 {{connection}} 占位符，无法保存 --database 配置。",
            ));
        }
        let connection = connection
            .map(|name| format!(".connection({name:?})"))
            .unwrap_or_default();
        let table_reference = if input.matches().get_flag("fields") {
            format!("{class}::TABLE")
        } else {
            quoted_table.clone()
        };
        Ok(Cow::Owned(
            stub.replace("{{model}}", class)
                .replace("{{table}}", &quoted_table[1..quoted_table.len() - 1])
                .replace("{{table_reference}}", &table_reference)
                .replace("{{connection}}", &connection),
        ))
    }
}

fn model_name(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("name required by Clap")
}

pub(super) fn validated_parts(name: &str) -> Result<Vec<&str>, ConsoleError> {
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
