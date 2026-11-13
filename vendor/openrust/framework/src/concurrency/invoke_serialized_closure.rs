use std::io::{self, Read as _};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    log::context::Context,
};

use super::{
    Registry,
    protocol::{self, Invocation, Outcome},
};

struct InvokeSerializedClosure(CommandDefinition);

impl Command for InvokeSerializedClosure {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            io::stdin()
                .lock()
                .take((protocol::INPUT_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            if bytes.len() > protocol::INPUT_LIMIT {
                return Err(error(super::Error::PayloadTooLarge));
            }
            let (task, context, key) = if input.matches().get_flag("detached") {
                let (task, context) = serde_json::from_slice(&bytes).map_err(error)?;
                (task, context, None)
            } else {
                let Invocation { task, context, key } =
                    serde_json::from_slice(&bytes).map_err(error)?;
                (task, context, Some(key))
            };
            let registry = application.container().make::<Registry>().map_err(error)?;
            let repository = Context::repository().map_err(error)?;
            Context::hooks()
                .map_err(error)?
                .hydrate(&context, &repository);
            let result = registry.invoke(task).await;
            let Some(key) = key else {
                return match result {
                    Ok(_) => Ok(0),
                    Err(error) => {
                        let _ = application.report(&error).await;
                        Ok(1)
                    }
                };
            };
            let outcome = match result {
                Ok(value) => Outcome::Success(value),
                Err(super::Error::TaskFailed { message, .. }) => Outcome::Failed(message),
                Err(failure) => Outcome::Failed(failure.to_string()),
            };
            output
                .stdout
                .push_str(&protocol::encode(&outcome, &key).map_err(error)?);
            Ok(0)
        })
    }
}

fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(InvokeSerializedClosure(CommandDefinition::new(
        clap::Command::new(protocol::COMMAND).hide(true).arg(
            clap::Arg::new("detached")
                .long("detached")
                .hide(true)
                .action(clap::ArgAction::SetTrue),
        ),
    ))))
}
