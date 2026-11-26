use std::time::{SystemTime, UNIX_EPOCH};

use openrust::{
    cache::CacheManager,
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::cache::Repository as _,
    foundation::Application,
};

use super::{RESTART, console_error};

pub(super) struct RestartServer(pub(super) CommandDefinition);

impl Command for RestartServer {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let cache = app
                .make::<CacheManager>()
                .await
                .map_err(console_error)?
                .store(None)
                .map_err(console_error)?;
            let signal = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(console_error)?
                .as_nanos()
                .to_string();
            if !cache
                .forever(RESTART, &openrust::cache::Value::from(signal.as_str()))
                .await
                .map_err(console_error)?
            {
                return Err(ConsoleError::new(
                    "Could not store the Reverb restart signal.",
                ));
            }
            output.comment("Broadcasting Reverb restart signal.");
            Ok(0)
        })
    }
}
