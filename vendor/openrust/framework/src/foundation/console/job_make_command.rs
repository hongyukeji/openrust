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

use super::test_make_command;

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (middleware, name, about) in [
        (false, "make:job", "Create a new job class"),
        (
            true,
            "make:job-middleware",
            "Create a new job middleware class",
        ),
    ] {
        let syntax = Syntax::new(name)
            .about(about)
            .arg(Arg::new("name").required(true))
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("test").long("test").action(ArgAction::SetTrue));
        let syntax = if middleware {
            syntax
        } else {
            syntax
                .arg(
                    Arg::new("sync")
                        .long("sync")
                        .action(ArgAction::SetTrue)
                        .help("Generate a synchronous job"),
                )
                .arg(
                    Arg::new("batched")
                        .long("batched")
                        .action(ArgAction::SetTrue)
                        .help("Generate a batchable job (requires queue batching support)"),
                )
        };
        artisan.add_command(Box::new(JobMakeCommand {
            definition: CommandDefinition::new(syntax),
            middleware,
        }))?;
    }
    Ok(())
}

struct JobMakeCommand {
    definition: CommandDefinition,
    middleware: bool,
}

impl Command for JobMakeCommand {
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
            let args = input.matches();
            let (namespace, stub, label) = if self.middleware {
                (
                    &["Jobs", "Middleware"][..],
                    Stub::JobMiddleware,
                    "Middleware",
                )
            } else {
                if args.get_flag("batched") {
                    return Err(ConsoleError::new("Queue batching is not implemented."));
                }
                let stub = if args.get_flag("sync") {
                    Stub::Job
                } else {
                    Stub::QueuedJob
                };
                (&["Jobs"][..], stub, "Job")
            };
            let name = args
                .get_one::<String>("name")
                .expect("name required by Clap");
            let (namespace, name) = match name
                .strip_prefix("App/")
                .or_else(|| name.strip_prefix("App\\"))
            {
                Some(name) => (&[][..], name),
                None => (namespace, name.as_str()),
            };
            let parts: Vec<_> = namespace
                .iter()
                .copied()
                .chain(source_name_parts(
                    name,
                    "Use a PascalCase job or middleware name.",
                )?)
                .collect();
            let class = parts.last().expect("validated name");
            let mut relative = PathBuf::from("app");
            relative.extend(parts.iter().map(|name| snake_case(name)));
            let relative = relative.with_extension("rs");
            let path = app.base_path().join(&relative);
            let directory_module = path.with_extension("").join("mod.rs");
            if directory_module
                .try_exists()
                .map_err(|error| ConsoleError::new(error.to_string()))?
            {
                return Err(ConsoleError::new(format!(
                    "{} already defines the module.",
                    directory_module.display()
                )));
            }

            let template = stub
                .load(app.base_path())
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let code = template.replace("{{class}}", class);
            let force = args.get_flag("force");
            tokio::task::spawn_blocking(move || write_generated(&path, code.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "{label} [{}] created successfully.",
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the module before using the generated type.\n");

            if args.get_flag("test") {
                let test = format!("{}Test", parts.join("/"));
                test_make_command::generate_matching(app, &test, force, output)
                    .await
                    .map_err(|error| ConsoleError::new(format!(
                        "{label} created. Could not generate its test: {error} Retry with make:test {test}."
                    )))?;
            }
            Ok(0)
        })
    }
}
