//! Artisan lifecycle for a bootstrapped application.

mod console_make_command;

use std::ffi::OsString;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::console,
    support::facades,
};

use super::{Application, BootstrapError};

/// Boots the application and runs Artisan commands.
pub struct Kernel {
    application: Application,
    artisan: Artisan,
    captured: Output,
    register_planned: bool,
    command_registration: Option<Result<(), ConsoleError>>,
}

impl Kernel {
    pub fn new(application: Application, artisan: Artisan) -> Self {
        Self {
            application,
            artisan,
            captured: Output::default(),
            register_planned: false,
            command_registration: None,
        }
    }

    pub(super) fn for_artisan(application: Application) -> Self {
        Self {
            register_planned: true,
            ..Self::new(application, Artisan::new())
        }
    }
}

impl console::Kernel for Kernel {
    fn bootstrap(&mut self) -> ConsoleFuture<'_, Result<(), BootstrapError>> {
        Box::pin(facades::scope(
            self.application.container_handle(),
            async move {
                self.application.boot().await?;
                self.command_registration
                    .get_or_insert_with(|| {
                        if self.register_planned {
                            self.artisan.register_planned_commands()?;
                        }
                        (self.application.commands)(&mut self.artisan)?;
                        if let Some(register) = self.application.command_routes {
                            facades::Artisan::register(&mut self.artisan, register)?;
                        }
                        Ok(())
                    })
                    .clone()
                    .map_err(|source| BootstrapError::CommandRegistrationFailed { source })
            },
        ))
    }

    fn get_application(&self) -> &Application {
        &self.application
    }

    fn all(&self) -> Vec<&CommandDefinition> {
        self.artisan.all()
    }

    fn handle<'a>(
        &'a mut self,
        arguments: &'a [OsString],
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, i32> {
        Box::pin(facades::scope(
            self.application.container_handle(),
            async move {
                if let Err(error) = self.bootstrap().await {
                    output.stderr.push_str("Application startup failed: ");
                    self.application.render_console_exception(output, &error);
                    return 1;
                }
                self.artisan
                    .handle(&self.application, arguments, output)
                    .await
            },
        ))
    }

    fn terminate<'a>(
        &'a mut self,
        _arguments: &'a [OsString],
        _status: i32,
    ) -> ConsoleFuture<'a, ()> {
        Box::pin(async {})
    }

    fn register_command(&mut self, command: Box<dyn Command>) -> Result<(), ConsoleError> {
        self.artisan.add_command(command)
    }

    fn call<'a>(
        &'a mut self,
        command: &'a str,
        input: &'a Input,
        output: Option<&'a mut Output>,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(facades::scope(
            self.application.container_handle(),
            async move {
                self.bootstrap()
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
                let output = match output {
                    Some(output) => output,
                    None => {
                        self.captured = Output::default();
                        &mut self.captured
                    }
                };
                self.artisan
                    .call(&self.application, command, input, output)
                    .await
            },
        ))
    }

    fn output(&mut self) -> String {
        std::mem::take(&mut self.captured.stdout)
    }
}
