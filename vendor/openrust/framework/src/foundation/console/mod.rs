//! Artisan lifecycle for a bootstrapped application.

mod channel_list_command;
pub(super) mod config_cache_command;
mod configuration_command;
mod console_make_command;
pub(super) mod dev_command;
mod dev_list_command;
mod dev_terminal;
mod environment_command;
mod environment_encryption_command;
mod event_cache_command;
mod event_clear_command;
mod event_list_command;
mod event_make_command;
mod init_command;
mod install_routes_command;
pub mod installation;
mod job_make_command;
mod key_generate_command;
mod lang_publish_command;
mod mail_make_command;
mod maintenance_command;
mod message_make_command;
mod model_fields_command;
mod model_make_command;
mod notification_make_command;
mod observer_make_command;
mod package_manifest_command;
mod policy_make_command;
mod provider_make_command;
mod queued_command;
mod resource_make_command;
pub(super) mod serve_command;
#[doc(hidden)]
pub mod source_edit;
mod storage_link_command;
mod storage_unlink_command;
mod stub_publish_command;
mod test_command;
mod test_make_command;
mod type_make_command;
mod vendor_publish_command;
mod view_component_make_command;
pub use install_routes_command::ApiInstallCommand;
pub use queued_command::PendingQueuedCommand;
pub(crate) use queued_command::QueuedCommand;
pub use vendor_publish_command::VendorPublishCommand;

use std::{
    ffi::OsString,
    future::Future,
    io::Write as _,
    sync::{Arc, Weak},
};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, scheduling,
    },
    contracts::{console, container::Container as _},
    support::facades,
};

use super::{
    Application, BootstrapError, Runner, RunnerContext, RuntimeResult,
    application::ApplicationState, environment_detector, runner::ServiceExit,
};
use tokio::sync::OnceCell;

struct ConsoleApplication(Weak<ApplicationState>);

pub(super) fn bind_application(application: &Arc<ApplicationState>) {
    installation::bind(application);
    application
        .container
        .instance(Arc::new(ConsoleApplication(Arc::downgrade(application))))
        .expect("Artisan application is initially unbound");
}

pub(crate) async fn call_in_application(
    command: &str,
    arguments: &[OsString],
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    let shared = facades::current_container()
        .and_then(|container| container.make::<ConsoleApplication>().ok())
        .and_then(|application| application.0.upgrade())
        .ok_or_else(|| ConsoleError::new("No active Artisan application."))?;
    let mut kernel = Kernel::for_artisan(Application { shared });
    facades::scope(kernel.application.container_handle(), async {
        kernel
            .bootstrap_with_environment(None)
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        let application = &kernel.application;
        within_console_lifecycle(
            application,
            kernel
                .artisan
                .call_arguments(application, command, arguments, output),
        )
        .await
    })
    .await
}

async fn within_console_lifecycle(
    application: &Application,
    invocation: impl Future<Output = Result<i32, ConsoleError>>,
) -> Result<i32, ConsoleError> {
    use crate::concurrency::DeferredCallbacks;

    if DeferredCallbacks::is_active() || crate::http::request_context::with_uri(|_| ()).is_some() {
        return invocation.await;
    }
    let deferred = DeferredCallbacks::default();
    let result = deferred.scope(invocation).await;
    deferred
        .invoke(
            application,
            result.as_ref().is_ok_and(|&status| status == 0),
        )
        .await;
    result
}

/// Boots the application and runs Artisan commands.
pub struct Kernel {
    application: Application,
    artisan: Artisan,
    captured: Output,
    register_planned: bool,
    command_registration: Option<Result<(), ConsoleError>>,
    has_scheduled_tasks: bool,
    deferred: crate::concurrency::DeferredCallbacks,
}

impl Kernel {
    pub fn new(application: Application, artisan: Artisan) -> Self {
        Self {
            application,
            artisan,
            captured: Output::default(),
            register_planned: false,
            command_registration: None,
            has_scheduled_tasks: false,
            deferred: Default::default(),
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
                self.application.load_deferred_providers().await?;
                self.command_registration
                    .get_or_insert_with(|| {
                        if self.register_planned {
                            self.artisan.register_planned_commands()?;
                            self.artisan.register_task_commands()?;
                            environment_command::register(&mut self.artisan)?;
                            config_cache_command::register(&mut self.artisan)?;
                            configuration_command::register(&mut self.artisan)?;
                            environment_encryption_command::register(&mut self.artisan)?;
                            event_cache_command::register(&mut self.artisan)?;
                            event_clear_command::register(&mut self.artisan)?;
                            event_list_command::register(&mut self.artisan)?;
                            event_make_command::register(&mut self.artisan)?;
                            job_make_command::register(&mut self.artisan)?;
                            channel_list_command::register(&mut self.artisan)?;
                            install_routes_command::register(&mut self.artisan)?;
                            init_command::register(&mut self.artisan)?;
                            console_make_command::register(&mut self.artisan)?;
                            test_make_command::register(&mut self.artisan)?;
                            type_make_command::register(&mut self.artisan)?;
                            test_command::register(&mut self.artisan)?;
                            stub_publish_command::register(&mut self.artisan)?;
                            model_make_command::register(&mut self.artisan)?;
                            model_fields_command::register(&mut self.artisan)?;
                            observer_make_command::register(&mut self.artisan)?;
                            policy_make_command::register(&mut self.artisan)?;
                            provider_make_command::register(&mut self.artisan)?;
                            resource_make_command::register(&mut self.artisan)?;
                            key_generate_command::register(&mut self.artisan)?;
                            lang_publish_command::register(&mut self.artisan)?;
                            mail_make_command::register(&mut self.artisan)?;
                            notification_make_command::register(&mut self.artisan)?;
                            maintenance_command::register(&mut self.artisan)?;
                            serve_command::register(&mut self.artisan)?;
                            dev_command::register(&mut self.artisan)?;
                            dev_list_command::register(&mut self.artisan)?;
                            crate::auth::console::register(&mut self.artisan)?;
                            #[cfg(feature = "sanctum")]
                            crate::sanctum::prune_expired::register(&mut self.artisan)?;
                            vendor_publish_command::register(&mut self.artisan)?;
                            package_manifest_command::register(&mut self.artisan)?;
                            crate::cache::console::register(&mut self.artisan)?;
                            crate::queue::console::register(&mut self.artisan)?;
                            crate::notifications::console::register(&mut self.artisan)?;
                            crate::concurrency::register(&mut self.artisan)?;
                            crate::view::console::register(&mut self.artisan)?;
                            view_component_make_command::register(&mut self.artisan)?;
                            crate::routing::console::route_list_command::register(
                                &mut self.artisan,
                            )?;
                            crate::routing::console::route_cache_command::register(
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
                            for configure in self
                                .application
                                .shared
                                .schedule_configurators
                                .lock()
                                .expect("调度配置锁中毒")
                                .iter()
                            {
                                scheduling::ScheduleFacade::configure(|schedule| {
                                    configure(schedule)
                                })?;
                            }
                            Ok(())
                        })?;
                        if self.register_planned {
                            self.has_scheduled_tasks = schedule.events().next().is_some();
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

pub(super) struct RunnerServiceProvider;

impl crate::support::ServiceProvider for RunnerServiceProvider {
    fn runners(&self, app: &Application) -> Vec<Arc<dyn Runner>> {
        let commands = Arc::new(OnceCell::new());
        vec![
            Arc::new(ManagedCommand {
                signature: "queue",
                name: "queue:work",
                options: &["--memory=0"],
                application: app.share(),
                commands: commands.clone(),
            }),
            Arc::new(ManagedCommand {
                signature: "schedule",
                name: "schedule:work",
                options: &[],
                application: app.share(),
                commands,
            }),
        ]
    }
}

struct ManagedCommand {
    signature: &'static str,
    name: &'static str,
    options: &'static [&'static str],
    application: Application,
    commands: Arc<OnceCell<Kernel>>,
}

impl ManagedCommand {
    async fn commands(&self) -> Result<&Kernel, BootstrapError> {
        self.commands
            .get_or_try_init(|| async {
                let mut kernel = Kernel::for_artisan(self.application.share());
                console::Kernel::bootstrap(&mut kernel).await?;
                Ok(kernel)
            })
            .await
    }
}

impl Runner for ManagedCommand {
    fn signature(&self) -> &str {
        self.signature
    }

    fn should_run(&self) -> crate::http::HttpFuture<'_, RuntimeResult<bool>> {
        Box::pin(async move {
            if self.name == "queue:work" {
                use crate::contracts::foundation::Application as _;
                let connection = self.application.config()?.get("queue.default");
                if !matches!(connection.as_ref().and_then(crate::config::Value::as_str), Some(name) if !name.is_empty())
                {
                    return Ok(false);
                }
                let manager = self
                    .application
                    .make::<crate::queue::QueueManager>()
                    .await?;
                Ok(!manager.default_is_synchronous()?)
            } else {
                Ok(self.commands().await?.has_scheduled_tasks)
            }
        })
    }

    fn run(&self, context: RunnerContext) -> crate::http::HttpFuture<'_, RuntimeResult<()>> {
        Box::pin(facades::scope(
            self.application.container_handle(),
            async move {
                let arguments: Vec<_> = std::iter::once(&self.name)
                    .chain(self.options)
                    .map(OsString::from)
                    .collect();
                let mut output = Output::for_terminal();
                let deferred = crate::concurrency::DeferredCallbacks::default();
                let status = deferred
                    .scope(self.commands().await?.artisan.handle_with_shutdown(
                        &self.application,
                        &arguments,
                        &mut output,
                        Some(context),
                    ))
                    .await;
                deferred.invoke(&self.application, status == 0).await;
                std::io::stdout().write_all(output.stdout.as_bytes())?;
                std::io::stderr().write_all(output.stderr.as_bytes())?;
                if status == 0 {
                    Ok(())
                } else {
                    Err(ServiceExit {
                        name: self.name,
                        status,
                    }
                    .into())
                }
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
        self.deferred = Default::default();
        Box::pin(facades::scope(
            self.application.container_handle(),
            async move {
                let selected = match environment_detector::console_environment(arguments) {
                    Ok(selected) => selected,
                    Err(error) => {
                        self.application
                            .render_console_exception(output, error)
                            .await;
                        return 1;
                    }
                };
                if let Err(error) = self.bootstrap_with_environment(selected).await {
                    output.stderr.push_str("Application startup failed: ");
                    self.application
                        .render_console_exception(output, error)
                        .await;
                    return 1;
                }
                self.deferred
                    .scope(self.artisan.handle(&self.application, arguments, output))
                    .await
            },
        ))
    }

    fn terminate<'a>(
        &'a mut self,
        _arguments: &'a [OsString],
        status: i32,
    ) -> ConsoleFuture<'a, ()> {
        Box::pin(facades::scope(
            self.application.container_handle(),
            self.deferred.invoke(&self.application, status == 0),
        ))
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
                within_console_lifecycle(
                    &self.application,
                    self.artisan.call(&self.application, command, input, output),
                )
                .await
            },
        ))
    }

    fn output(&mut self) -> String {
        std::mem::take(&mut self.captured.stdout)
    }
}
