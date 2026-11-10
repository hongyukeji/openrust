use std::ffi::OsString;

use crate::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::{Application, BootstrapError},
};

/// Bootstraps Artisan, dispatches commands, and terminates the application.
pub trait Kernel: Send + Sync {
    fn bootstrap(&mut self) -> ConsoleFuture<'_, Result<(), BootstrapError>>;
    fn get_application(&self) -> &Application;
    fn all(&self) -> Vec<&CommandDefinition>;
    fn handle<'a>(
        &'a mut self,
        arguments: &'a [OsString],
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, i32>;
    fn terminate<'a>(&'a mut self, arguments: &'a [OsString], status: i32)
    -> ConsoleFuture<'a, ()>;
    fn register_command(&mut self, command: Box<dyn Command>) -> Result<(), ConsoleError>;
    fn call<'a>(
        &'a mut self,
        command: &'a str,
        input: &'a Input,
        output: Option<&'a mut Output>,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>;
    fn output(&mut self) -> String;
}
