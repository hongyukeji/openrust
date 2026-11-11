use std::path::PathBuf;

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        generator_command::{snake_case, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(FactoryMakeCommand(CommandDefinition::new(
        Syntax::new("make:factory")
            .about("Create a new Eloquent model factory")
            .arg(Arg::new("name").required(true))
            .arg(Arg::new("model").long("model"))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue)),
    ))))
}

struct FactoryMakeCommand(CommandDefinition);

impl Command for FactoryMakeCommand {
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
            let arguments = input.matches();
            let name = arguments
                .get_one::<String>("name")
                .expect("required by Clap");
            let parts = type_parts(name)?;
            let (class, namespaces) = parts.split_last().expect("validated name");
            let class = if class.ends_with("Factory") {
                (*class).to_owned()
            } else {
                format!("{class}Factory")
            };
            let model = arguments
                .get_one::<String>("model")
                .map_or_else(|| class.trim_end_matches("Factory"), String::as_str);
            let model = model_import(model)?;
            let mut relative = PathBuf::from("database/factories");
            relative.extend(namespaces.iter().map(|part| snake_case(part)));
            relative.push(format!("{}.rs", snake_case(&class)));
            let path = app.base_path().join(&relative);
            let code = format!(
                "use openrust::database::eloquent::{{Attributes, factories::Factory}};\n\nuse {model};\n\npub struct {class};\n\nimpl {class} {{\n    pub fn factory() -> Factory<{}> {{\n        Factory::new(Self::definition)\n    }}\n\n    fn definition() -> Attributes {{\n        Attributes::new()\n    }}\n}}\n",
                model.rsplit("::").next().expect("validated model"),
            );
            let force = arguments.get_flag("force");
            tokio::task::spawn_blocking(move || write_generated(&path, code.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "Factory [{}] created successfully.",
                relative.display()
            ));
            Ok(0)
        })
    }
}

fn type_parts(name: &str) -> Result<Vec<&str>, ConsoleError> {
    let parts: Vec<_> = name.split(['/', '\\']).collect();
    if parts.iter().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || matches!(
                snake_case(part).as_str(),
                "self" | "super" | "crate" | "mod"
            )
    }) {
        return Err(ConsoleError::new("Use a PascalCase model or factory name."));
    }
    Ok(parts)
}

fn model_import(name: &str) -> Result<String, ConsoleError> {
    let parts = type_parts(name)?;
    let class = *parts.last().expect("validated name");
    let mut import = String::from("crate::app::models");
    for part in parts {
        import.push_str("::");
        import.push_str(&snake_case(part));
    }
    import.push_str("::");
    import.push_str(class);
    Ok(import)
}
