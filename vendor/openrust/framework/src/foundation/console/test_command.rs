use std::{ffi::OsString, io};

use clap::{Arg, Command as Syntax, value_parser};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::Application,
    process::{OutputStream, Process},
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(TestCommand(CommandDefinition::new(
        Syntax::new("test")
            .about("Run the application test suite")
            .arg(
                Arg::new("arguments")
                    .value_name("CARGO_ARGS")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true)
                    .value_parser(value_parser!(OsString))
                    .help("Arguments for cargo test"),
            ),
    ))))
}

struct TestCommand(CommandDefinition);

impl Command for TestCommand {
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
            let mut command = vec![
                std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
                "test".into(),
            ];
            command.extend(
                args.get_many::<OsString>("arguments")
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            let process = Process::command(command)
                .map_err(process_error)?
                .path(app.base_path())
                .forever();

            if output.streams_to_terminal() {
                let silent = args
                    .try_get_one::<bool>("silent")
                    .ok()
                    .flatten()
                    .copied()
                    .unwrap_or(false);
                let quiet = args
                    .try_get_one::<bool>("quiet")
                    .ok()
                    .flatten()
                    .copied()
                    .unwrap_or(false);
                let run = process.runWithOutput(move |stream, bytes| {
                    if silent {
                        return;
                    }
                    match stream {
                        OutputStream::Stdout if !quiet => write_stream(io::stdout().lock(), bytes),
                        OutputStream::Stderr => write_stream(io::stderr().lock(), bytes),
                        OutputStream::Stdout => {}
                    }
                });
                return tokio::select! {
                    result = run => Ok(result.map_err(process_error)?.exitCode()),
                    result = tokio::signal::ctrl_c() => {
                        result.map_err(|error| ConsoleError::new(error.to_string()))?;
                        Ok(130)
                    }
                };
            }

            let result = process.run().await.map_err(process_error)?;
            output
                .stdout
                .push_str(&String::from_utf8_lossy(result.output()));
            output
                .stderr
                .push_str(&String::from_utf8_lossy(result.errorOutput()));
            Ok(result.exitCode())
        })
    }
}

fn process_error(error: crate::process::Error) -> ConsoleError {
    ConsoleError::new(format!("Unable to run cargo test: {error}"))
}

fn write_stream(mut stream: impl io::Write, bytes: &[u8]) {
    let _ = stream.write_all(bytes);
    let _ = stream.flush();
}
