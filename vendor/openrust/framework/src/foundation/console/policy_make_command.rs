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

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(PolicyMakeCommand(CommandDefinition::new(
        Syntax::new("make:policy")
            .about("Create a new authorization policy")
            .arg(Arg::new("name").required(true))
            .arg(Arg::new("model").long("model"))
            .arg(Arg::new("force").long("force").action(ArgAction::SetTrue)),
    ))))
}

struct PolicyMakeCommand(CommandDefinition);

impl Command for PolicyMakeCommand {
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
            let name = args.get_one::<String>("name").expect("required by Clap");
            let parts = names(name)?;
            let (policy, namespace) = parts.split_last().expect("validated name");
            let mut relative = PathBuf::from("app/policies");
            relative.extend(namespace.iter().map(|part| snake_case(part)));
            relative.push(format!("{}.rs", snake_case(policy)));
            let path = app.base_path().join(&relative);
            let (model, declaration, abilities) = match args.get_one::<String>("model") {
                Some(model) => {
                    let parts = names(model)?;
                    let (class, modules) = parts.split_last().expect("validated model name");
                    let mut import = String::from("crate::app::models::");
                    for module in modules {
                        import.push_str(&snake_case(module));
                        import.push_str("::");
                    }
                    import.push_str(class);
                    (
                        (*class).to_owned(),
                        format!("use {import};"),
                        r#"matches!(ability, "viewAny" | "view" | "create" | "update" | "delete" | "restore" | "forceDelete")"#,
                    )
                }
                None => {
                    let model = format!("{policy}Target");
                    let declaration = format!("pub struct {model};");
                    (model, declaration, "false")
                }
            };
            let code = include_str!("stubs/policy.rs.stub")
                .replace("{{declaration}}", &declaration)
                .replace("{{policy}}", policy)
                .replace("{{model}}", &model)
                .replace("{{abilities}}", abilities);
            let force = args.get_flag("force");
            tokio::task::spawn_blocking(move || write_generated(&path, code.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "Policy [{}] created successfully.",
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the policy module and register it with Gate::policy.\n");
            Ok(0)
        })
    }
}

fn names(name: &str) -> Result<Vec<&str>, ConsoleError> {
    let parts: Vec<_> = name.split(['/', '\\']).collect();
    if parts.iter().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || matches!(
                snake_case(part).as_str(),
                "self" | "super" | "crate" | "mod"
            )
    }) {
        return Err(ConsoleError::new("Use a PascalCase policy or model name."));
    }
    Ok(parts)
}
