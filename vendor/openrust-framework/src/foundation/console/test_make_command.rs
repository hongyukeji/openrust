use std::{borrow::Cow, path::PathBuf};

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
    artisan.add_command(Box::new(TestMakeCommand::new()))
}

pub(super) async fn generate_matching(
    app: &Application,
    name: &str,
    force: bool,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    let generator = TestMakeCommand::new();
    let mut arguments = vec![name.into()];
    if force {
        arguments.push("--force".into());
    }
    let input = generator.definition().parse(&arguments)?;
    generator.handle(app, &input, output).await?;
    Ok(())
}

struct TestMakeCommand(CommandDefinition);

impl TestMakeCommand {
    fn new() -> Self {
        Self(CommandDefinition::new(
            Syntax::new("make:test")
                .about("Create a new test")
                .arg(Arg::new("name").required(true).help("The name of the test"))
                .arg(
                    Arg::new("unit")
                        .short('u')
                        .long("unit")
                        .help("Create a unit test")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .help("Replace an existing test file")
                        .action(ArgAction::SetTrue),
                ),
        ))
    }
}

impl Command for TestMakeCommand {
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
            let path = self.get_path(app, test_name(input), input)?;
            let relative = path
                .strip_prefix(app.base_path())
                .unwrap_or(&path)
                .to_owned();
            let code = self.get_stub(app, input)?;
            let force = input.matches().get_flag("force");
            tokio::task::spawn_blocking(move || write_generated(&path, code.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "Test [{}] created successfully.",
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the new module in the Cargo test target before running it.\n");
            Ok(0)
        })
    }
}

impl GeneratorCommand for TestMakeCommand {
    fn get_path(
        &self,
        app: &Application,
        name: &str,
        input: &Input,
    ) -> Result<PathBuf, ConsoleError> {
        let directory = if input.matches().get_flag("unit") {
            "unit"
        } else {
            "feature"
        };
        let mut path = app.base_path().join("tests").join(directory);
        path.extend(validated_parts(name)?.map(snake_case));
        let path = path.with_extension("rs");
        if path
            == app
                .base_path()
                .join("tests")
                .join(directory)
                .join("main.rs")
        {
            return Err(ConsoleError::new("The Cargo test entry point is reserved."));
        }
        Ok(path)
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let class = validated_parts(test_name(input))?
            .last()
            .expect("validated name");
        let stub = if input.matches().get_flag("unit") {
            Stub::UnitTest
        } else {
            Stub::Test
        };
        let template = stub
            .load(app.base_path())
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        Ok(Cow::Owned(template.replace(
            "{{test}}",
            &format!("test_{}", snake_case(class)),
        )))
    }
}

fn test_name(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("name required by Clap")
}

fn validated_parts(name: &str) -> Result<impl Iterator<Item = &str>, ConsoleError> {
    source_name_parts(name, "Use a PascalCase test name, such as UserTest.")
}
