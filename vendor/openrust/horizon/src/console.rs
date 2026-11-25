use crate::{Error, MasterSupervisor, Repository};
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::facades::Artisan,
};
use tokio_util::sync::CancellationToken;
mod install;
mod listen;
mod management;
mod master_control;
mod process_output;
mod purge;
mod supervisor_command;
mod work_command;
pub use work_command::WorkCommand;
struct HorizonCommand(CommandDefinition, &'static str);
pub(crate) fn register() -> std::result::Result<(), ConsoleError> {
    Artisan::add(Box::new(WorkCommand::new()))?;
    Artisan::add(Box::new(supervisor_command::SupervisorCommand::new()))?;
    management::register()?;
    for name in [
        "horizon",
        "horizon:status",
        "horizon:pause",
        "horizon:continue",
        "horizon:terminate",
        "horizon:failed",
        "horizon:retry",
        "horizon:forget",
    ] {
        let mut syntax = Syntax::new(name).about(match name {
            "horizon" => "Start the Horizon master supervisor",
            "horizon:status" => "Show active Horizon supervisors",
            "horizon:pause" => "Pause the master supervisor",
            "horizon:continue" => "Instruct the master supervisor to continue processing jobs",
            "horizon:terminate" => "Terminate the master supervisor so it can be restarted",
            "horizon:failed" => "List recent sanitized Horizon failures",
            "horizon:retry" => "Retry one stored failure from a configured Horizon queue",
            _ => "Forget one stored failure from a configured Horizon queue",
        });
        if name == "horizon:retry" {
            syntax = syntax.arg(Arg::new("id").required(true));
        }
        if name == "horizon:forget" {
            syntax = syntax
                .arg(Arg::new("id"))
                .arg(Arg::new("all").long("all").action(ArgAction::SetTrue));
        }
        if name == "horizon" {
            syntax = syntax.arg(Arg::new("environment").long("environment"));
        }
        if name == "horizon:terminate" {
            syntax = syntax.arg(
                Arg::new("wait")
                    .long("wait")
                    .action(ArgAction::SetTrue)
                    .help("Wait for all workers to terminate"),
            );
        }
        if name == "horizon:status" {
            syntax = syntax.arg(Arg::new("json").long("json").action(ArgAction::SetTrue));
        }
        Artisan::add(Box::new(HorizonCommand(
            CommandDefinition::new(syntax),
            name,
        )))?;
    }
    Ok(())
}
fn error(_: impl std::fmt::Debug) -> ConsoleError {
    ConsoleError::new("Horizon operation failed; check configuration and service availability.")
}
impl Command for HorizonCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, std::result::Result<i32, ConsoleError>> {
        Box::pin(async move {
            let repository = app.make::<Repository>().await.map_err(error)?;
            match self.1 {
                "horizon" => {
                    let config = app.config().map_err(error)?;
                    let environment = input
                        .matches()
                        .get_one::<String>("environment")
                        .cloned()
                        .or_else(|| {
                            config
                                .get("horizon.env")
                                .and_then(|v| v.as_str().map(str::to_owned))
                        })
                        .or_else(|| {
                            config
                                .get("app.env")
                                .and_then(|v| v.as_str().map(str::to_owned))
                        })
                        .ok_or_else(|| error(Error::InvalidConfiguration))?;
                    let queues = app
                        .make::<openrust::queue::QueueManager>()
                        .await
                        .map_err(error)?;
                    let supervisor = MasterSupervisor::new(
                        repository,
                        queues,
                        Default::default(),
                        std::env::current_exe().map_err(error)?,
                        app.base_path().to_owned(),
                        environment.clone(),
                    )
                    .map_err(error)?
                    .fast_termination(
                        config
                            .get("horizon.fast_termination")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false),
                    );
                    let deployment =
                        crate::ProvisioningPlan::new(supervisor.name(), config.as_ref())
                            .and_then(|plan| plan.deployment(&environment, config.as_ref()))
                            .map_err(error)?;
                    let child_output = process_output::CommandOutput::new(output);
                    let supervisor = supervisor
                        .with_deployment(deployment)
                        .with_output_handler(child_output.handler());
                    let shutdown = input
                        .shutdown()
                        .cloned()
                        .unwrap_or_else(CancellationToken::new);
                    let signals = signals().map_err(error)?;
                    let result = {
                        let run = supervisor.run_with_start(&shutdown, || {
                            process_output::info(output, "Horizon started successfully.");
                        });
                        tokio::pin!(run);
                        tokio::select! {
                            result = &mut run => result,
                            () = signals => { shutdown.cancel(); run.await }
                        }
                    };
                    child_output.finish(output);
                    if matches!(result, Err(Error::MasterAlreadyRunning)) {
                        output.warning(&Error::MasterAlreadyRunning.to_string());
                        return Ok(0);
                    }
                    if matches!(result, Err(Error::MemoryLimitExceeded)) {
                        output.error(
                            "Memory limit exceeded. Consider increasing horizon.memory_limit.",
                        );
                        return Ok(12);
                    }
                    result.map_err(error)?;
                }
                "horizon:pause" | "horizon:continue" | "horizon:terminate" => {
                    let control = match self.1 {
                        "horizon:pause" => master_control::Control::Pause,
                        "horizon:continue" => master_control::Control::Continue,
                        _ => master_control::Control::Terminate {
                            wait: input.matches().get_flag("wait"),
                        },
                    };
                    control.handle(app, &repository, output).await?;
                }
                "horizon:status" => {
                    let masters = repository.master_supervisors().all().await.map_err(error)?;
                    let status = if masters.is_empty() {
                        2
                    } else if masters.iter().any(|master| master["status"] == "paused") {
                        1
                    } else {
                        0
                    };
                    if input.matches().get_flag("json") {
                        output
                            .stdout
                            .push_str(&serde_json::to_string(&masters).map_err(error)?);
                        output.stdout.push('\n');
                    } else {
                        match status {
                            2 => output.error("Horizon is inactive."),
                            1 => output.warning("Horizon is paused."),
                            _ => output.info("Horizon is running."),
                        }
                    }
                    return Ok(status);
                }
                "horizon:failed" => {
                    let records: Vec<_> = repository
                        .recent(100)
                        .await
                        .map_err(error)?
                        .into_iter()
                        .filter(|r| r.status == "failed")
                        .collect();
                    output
                        .stdout
                        .push_str(&serde_json::to_string(&records).map_err(error)?);
                    output.stdout.push('\n');
                }
                action => {
                    if action == "horizon:forget" {
                        return management::forget(app, input, output, &repository).await;
                    }
                    let id = input
                        .matches()
                        .get_one::<String>("id")
                        .expect("Clap required id");
                    let id = uuid::Uuid::parse_str(id).map_err(error)?.to_string();
                    let provider = app
                        .container()
                        .get::<dyn openrust::queue::failed::FailedJobProviderInterface>(
                            "queue.failer",
                        )
                        .map_err(error)?;
                    let monitored = repository
                        .jobs()
                        .find_failed(&id)
                        .await
                        .map_err(error)?
                        .ok_or_else(|| error(Error::InvalidRecord))?;
                    let failure_id = monitored.payload_uuid().map_err(error)?;
                    if repository
                        .jobs()
                        .find_failed_by_uuid(failure_id)
                        .await
                        .map_err(error)?
                        .is_none_or(|current| current.record.id != id)
                    {
                        return Err(error(Error::InvalidRecord));
                    }
                    let record = provider
                        .find(failure_id)
                        .await
                        .map_err(error)?
                        .ok_or_else(|| error(Error::InvalidRecord))?;
                    if record.connection != monitored.record.connection
                        || record.queue != monitored.record.queue
                        || serde_json::to_value(&record.payload).map_err(error)?
                            != monitored.payload
                    {
                        return Err(error(Error::InvalidRecord));
                    }
                    let config = app.config().map_err(error)?;
                    if config
                        .get(&format!("queue.connections.{}.driver", record.connection))
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .as_deref()
                        != Some("redis")
                    {
                        return Err(error(Error::InvalidRecord));
                    }
                    let manager = app
                        .make::<openrust::queue::QueueManager>()
                        .await
                        .map_err(error)?;
                    if let Some(retry_id) = manager
                        .retry_failed_with_reference(
                            provider.as_ref(),
                            failure_id,
                            &record,
                            &id,
                            crate::JobPayloadMetadata::retry_deadline(&record.payload)
                                .map_err(error)?,
                        )
                        .await
                        .map_err(error)?
                    {
                        repository
                            .jobs()
                            .store_retry_reference(&id, &retry_id)
                            .await
                            .map_err(error)?;
                        output.info("The failed job has been pushed back onto the queue!");
                    }
                    return Ok(0);
                }
            }
            Ok(0)
        })
    }
}

fn signals() -> std::io::Result<impl std::future::Future<Output = ()>> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(windows)]
    let mut terminate = tokio::signal::windows::ctrl_break()?;
    Ok(async move {
        #[cfg(any(unix, windows))]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
        #[cfg(not(any(unix, windows)))]
        let _ = tokio::signal::ctrl_c().await;
    })
}
