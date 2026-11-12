use std::{borrow::Cow, fs, io::ErrorKind, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        GeneratorCommand, Input, Output,
        generator_command::{snake_case, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
    support::Str,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ConsoleMakeCommand(CommandDefinition::new(
        Syntax::new("make:command")
            .about("Create a new Artisan command")
            .arg(
                Arg::new("name")
                    .required(true)
                    .help("The name of the command type"),
            )
            .arg(
                Arg::new("command")
                    .long("command")
                    .help("The terminal command name"),
            )
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .help("Replace an existing command file")
                    .action(ArgAction::SetTrue),
            ),
    ))))
}

struct ConsoleMakeCommand(CommandDefinition);

impl Command for ConsoleMakeCommand {
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
            let path = self.get_path(app, command_type(input), input)?;
            let relative = path
                .strip_prefix(app.base_path())
                .unwrap_or(&path)
                .to_owned();
            let force = input.matches().get_flag("force");
            let code = self.get_stub(app, input)?;
            tokio::task::spawn_blocking(move || write_generated(&path, code.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "Console command [{}] created successfully.",
                relative.display()
            ));
            output.stdout.push_str(
                "Declare the module and register the command in app/console/commands/mod.rs.\n",
            );
            Ok(0)
        })
    }
}

impl GeneratorCommand for ConsoleMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        let mut path = app.base_path().join("app/console/commands");
        path.extend(validated_parts(name)?.map(snake_case));
        Ok(path.with_extension("rs"))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let class = validated_parts(command_type(input))?
            .last()
            .expect("validated name");
        let command = input
            .matches()
            .get_one::<String>("command")
            .filter(|command| !command.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("app:{}", Str::kebab(class)));
        if !command.split(':').all(|part| {
            part.as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }) {
            return Err(ConsoleError::new(
                "Use a command name such as app:send-report.",
            ));
        }

        let custom = app.base_path().join("stubs/console.rs.stub");
        let template = match fs::read_to_string(&custom) {
            Ok(template) => Cow::Owned(template),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Cow::Borrowed(include_str!("stubs/console.rs.stub"))
            }
            Err(error) => return Err(ConsoleError::new(format!("{}: {error}", custom.display()))),
        };
        Ok(Cow::Owned(
            template
                .replace("{{class}}", class)
                .replace("{{command}}", &command),
        ))
    }
}

fn command_type(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("name required by Clap")
}

fn validated_parts(name: &str) -> Result<impl Iterator<Item = &str>, ConsoleError> {
    let parts = name.strip_suffix(".rs").unwrap_or(name).split(['/', '\\']);
    if parts.clone().any(|part| {
        !part.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || matches!(
                snake_case(part).as_str(),
                "self" | "super" | "crate" | "mod"
            )
    }) {
        return Err(ConsoleError::new(
            "Use a PascalCase command type, such as SendReport.",
        ));
    }
    Ok(parts)
}
