//! Artisan lifecycle for a bootstrapped application.

mod channel_list_command;
mod console_make_command;
mod environment_command;
mod event_list_command;
mod key_generate_command;
mod model_make_command;
mod policy_make_command;
mod storage_link_command;
mod storage_unlink_command;

use std::ffi::OsString;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, scheduling,
    },
    contracts::console,
    support::facades,
};

use super::{Application, BootstrapError, environment_detector};

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
    fn bootstrap_with_environment<'a>(
        &'a mut self,
        selected: Option<&'a str>,
    ) -> ConsoleFuture<'a, Result<(), BootstrapError>> {
        Box::pin(facades::scope(
            self.application.container_handle(),
            async move {
                self.application.boot_with_environment(selected).await?;
                self.application
                    .loadDeferredProviders()
                    .await
                    .map_err(|source| BootstrapError::ContainerFailed { source })?;
                self.command_registration
                    .get_or_insert_with(|| {
                        if self.register_planned {
                            self.artisan.register_planned_commands()?;
                            environment_command::register(&mut self.artisan)?;
                            event_list_command::register(&mut self.artisan)?;
                            channel_list_command::register(&mut self.artisan)?;
                            model_make_command::register(&mut self.artisan)?;
                            policy_make_command::register(&mut self.artisan)?;
                            key_generate_command::register(&mut self.artisan)?;
                            crate::auth::console::register(&mut self.artisan)?;
                            crate::cache::console::register(&mut self.artisan)?;
                            crate::queue::console::register(&mut self.artisan)?;
                            crate::notifications::console::register(&mut self.artisan)?;
                            crate::concurrency::register(&mut self.artisan)?;
                            crate::view::console::register(&mut self.artisan)?;
                            crate::routing::console::route_list_command::register(
                                &mut self.artisan,
                            )?;
                            storage_link_command::register(&mut self.artisan)?;
                            storage_unlink_command::register(&mut self.artisan)?;
                            crate::database::console::factories::register(&mut self.artisan)?;
                            crate::database::console::migrations::register(&mut self.artisan)?;
                            crate::database::console::seeds::register(&mut self.artisan)?;
                        }
                        let schedule = scheduling::Schedule::collect(|| {
                            let package_commands = self
                                .application
                                .shared
                                .package_commands
                                .lock()
                                .expect("package command list lock poisoned")
                                .clone();
                            for register in package_commands {
                                facades::Artisan::register(&mut self.artisan, register)?;
                            }
                            (self.application.shared.commands)(&mut self.artisan)?;
                            if let Some(register) = self.application.shared.command_routes {
                                facades::Artisan::register(&mut self.artisan, register)?;
                            }
                            Ok(())
                        })?;
                        if self.register_planned {
                            scheduling::register(&mut self.artisan, schedule)?;
                        }
                        Ok(())
                    })
                    .clone()
                    .map_err(|source| BootstrapError::CommandRegistrationFailed { source })
            },
        ))
    }
}

impl console::Kernel for Kernel {
    fn bootstrap(&mut self) -> ConsoleFuture<'_, Result<(), BootstrapError>> {
        self.bootstrap_with_environment(None)
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
                let selected = match environment_detector::console_environment(arguments) {
                    Ok(selected) => selected,
                    Err(error) => {
                        self.application
                            .render_console_exception(output, &error)
                            .await;
                        return 1;
                    }
                };
                if let Err(error) = self.bootstrap_with_environment(selected).await {
                    output.stderr.push_str("Application startup failed: ");
                    self.application
                        .render_console_exception(output, &error)
                        .await;
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
