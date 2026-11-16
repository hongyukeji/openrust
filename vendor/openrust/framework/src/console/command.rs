use std::{ffi::OsString, sync::Arc};

use crate::foundation::Application;

use super::{ConsoleError, ConsoleFuture, Input, Output};

/// Describes a command's name, aliases, arguments, options, and help.
#[derive(Clone)]
pub struct CommandDefinition {
    pub(super) syntax: Arc<clap::Command>,
    pub(super) isolated: bool,
}

impl CommandDefinition {
    pub fn new(syntax: clap::Command) -> Self {
        Self {
            syntax: Arc::new(syntax),
            isolated: false,
        }
    }

    /// Enables the optional `--isolated` switch for this command.
    pub fn isolatable(mut self) -> Self {
        if !self.isolated {
            let syntax = Arc::make_mut(&mut self.syntax);
            *syntax = std::mem::take(syntax).arg(
                clap::Arg::new("isolated")
                    .long("isolated")
                    .num_args(0..=1)
                    .default_missing_value("0")
                    .value_parser(clap::value_parser!(i32))
                    .help("Do not run the command if another instance is already running"),
            );
            self.isolated = true;
        }
        self
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
            shutdown: None,
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
#[derive(Clone)]
pub struct ClosureCommand {
    pub(super) definition: CommandDefinition,
    pub(super) callback: Arc<dyn Fn(&mut Output) + Send + Sync>,
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
