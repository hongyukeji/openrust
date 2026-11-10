use std::{ffi::OsString, sync::Arc};

use crate::foundation::Application;

use super::{ConsoleError, ConsoleFuture, Input, Output};

/// Describes a command's name, aliases, arguments, options, and help.
pub struct CommandDefinition {
    pub(super) syntax: Arc<clap::Command>,
}

impl CommandDefinition {
    pub fn new(syntax: clap::Command) -> Self {
        Self {
            syntax: Arc::new(syntax),
        }
    }

    pub fn name(&self) -> &str {
        self.syntax.get_name()
    }

    pub fn parse(&self, arguments: &[OsString]) -> Result<Input, ConsoleError> {
        let command = self.name().to_owned();
        let matches = self
            .syntax
            .as_ref()
            .clone()
            .try_get_matches_from(
                std::iter::once(OsString::from(&command)).chain(arguments.iter().cloned()),
            )
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        Ok(Input {
            syntax: self.syntax.clone(),
            matches,
        })
    }
}

/// An Artisan command with a definition shared by dispatch, list, and help.
pub trait Command: Send + Sync {
    fn definition(&self) -> &CommandDefinition;

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>;
}

/// A closure-based Artisan command registered by the application.
pub struct ClosureCommand {
    pub(super) definition: CommandDefinition,
    pub(super) callback: Box<dyn Fn(&mut Output) + Send + Sync>,
}

impl ClosureCommand {
    pub(crate) fn registration_key(&self) -> std::sync::Weak<clap::Command> {
        Arc::downgrade(&self.definition.syntax)
    }

    /// Sets the command description shown by Artisan help and list.
    pub fn purpose(&mut self, description: &'static str) -> &mut Self {
        let syntax = self.definition.syntax.as_ref().clone().about(description);
        self.definition = CommandDefinition::new(syntax);
        self
    }
}

impl Command for ClosureCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        _app: &'a Application,
        _input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            (self.callback)(output);
            Ok(0)
        })
    }
}
