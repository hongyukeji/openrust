use super::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output};
use crate::{
    cache::{CacheError, Ttl},
    contracts::console::Isolatable,
    foundation::Application,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) struct IsolatedCommand {
    command: Box<dyn Isolatable>,
    definition: CommandDefinition,
}
impl IsolatedCommand {
    pub(crate) fn new(command: Box<dyn Isolatable>) -> Self {
        let definition = command.definition().clone().isolatable();
        Self {
            command,
            definition,
        }
    }
}
impl Command for IsolatedCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn isolation(&self) -> Option<&dyn Isolatable> {
        Some(self.command.as_ref())
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        self.command.handle(app, input, output)
    }
}
pub(super) fn lease(ttl: Ttl) -> Duration {
    let seconds = match ttl {
        Ttl::Seconds(seconds) => u64::try_from(seconds).unwrap_or(0),
        Ttl::Until(until) => {
            let seconds = |time: SystemTime| {
                time.duration_since(UNIX_EPOCH)
                    .map_or(0, |value| value.as_secs())
            };
            seconds(until).saturating_sub(seconds(SystemTime::now()))
        }
    };
    Duration::from_secs(seconds)
}
#[derive(Debug)]
pub(super) struct LockError(pub(super) CacheError);
impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Command isolation lock: {}", self.0)
    }
}
impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
