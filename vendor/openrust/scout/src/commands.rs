use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    support::facades::Artisan,
};
struct ScoutCommand(CommandDefinition, bool);
pub(crate) fn register() -> Result<(), ConsoleError> {
    for (name, import, description) in [
        (
            "scout:import",
            true,
            "Import the given model into the search index",
        ),
        (
            "scout:flush",
            false,
            "Flush all of the model's records from the index",
        ),
    ] {
        let mut syntax = Syntax::new(name)
            .about(description)
            .arg(Arg::new("model").required(true));
        if import {
            syntax = syntax
                .arg(
                    Arg::new("chunk")
                        .short('c')
                        .long("chunk")
                        .value_parser(clap::value_parser!(u64)),
                )
                .arg(Arg::new("fresh").long("fresh").action(ArgAction::SetTrue));
        }
        Artisan::add(Box::new(ScoutCommand(
            CommandDefinition::new(syntax),
            import,
        )))?;
    }
    Ok(())
}
fn error(e: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(e.to_string())
}
impl Command for ScoutCommand {
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
            let manager = app.make::<crate::EngineManager>().await.map_err(error)?;
            let name = input
                .matches()
                .get_one::<String>("model")
                .expect("required model");
            let registration = manager
                .models
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| error(format!("Scout model [{name}] is not registered")))?;
            if self.1 {
                if input.matches().get_flag("fresh") {
                    (registration.flush)().await.map_err(error)?;
                }
                let count = (registration.import)(input.matches().get_one::<u64>("chunk").copied())
                    .await
                    .map_err(error)?;
                output.info(&format!("[{name}] models imported: {count}"));
            } else {
                (registration.flush)().await.map_err(error)?;
                output.info(&format!("All [{name}] records have been flushed."));
            }
            Ok(0)
        })
    }
}
