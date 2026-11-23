use super::error;
use crate::{Repository, SupervisorOptions};
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::facades::Artisan,
};

struct Management(CommandDefinition, &'static str);
pub(super) fn register() -> Result<(), ConsoleError> {
    for name in [
        "horizon:list",
        "horizon:supervisors",
        "horizon:supervisor-status",
        "horizon:pause-supervisor",
        "horizon:continue-supervisor",
        "horizon:purge",
        "horizon:timeout",
        "horizon:clear",
        "horizon:snapshot",
        "horizon:clear-metrics",
        "horizon:install",
        "horizon:publish",
        "horizon:listen",
    ] {
        let mut syntax = Syntax::new(name).about(match name {
            "horizon:list" => "List all of the deployed machines",
            "horizon:supervisors" => "List all of the supervisors",
            "horizon:supervisor-status" => "Show the status for a given supervisor",
            "horizon:pause-supervisor" => "Pause a supervisor",
            "horizon:continue-supervisor" => "Instruct the supervisor to continue processing jobs",
            "horizon:purge" => "Terminate any rogue Horizon processes",
            "horizon:timeout" => "Get the maximum timeout for the given environment",
            "horizon:clear" => "Delete all of the jobs from the specified queue",
            "horizon:snapshot" => "Store a snapshot of the queue metrics",
            "horizon:clear-metrics" => "Delete metrics for all jobs and queues",
            "horizon:install" => "Install all of the Horizon resources",
            "horizon:listen" => "Run Horizon and automatically restart workers on file changes",
            _ => "Publish all of the Horizon resources",
        });
        if matches!(
            name,
            "horizon:supervisor-status"
                | "horizon:pause-supervisor"
                | "horizon:continue-supervisor"
        ) {
            syntax = syntax.arg(Arg::new("name").required(true));
        }
        if name == "horizon:purge" {
            syntax = syntax.arg(Arg::new("signal").long("signal").default_value("SIGTERM"));
        }
        if name == "horizon:timeout" {
            syntax = syntax
                .hide(true)
                .arg(Arg::new("environment").default_value("production"));
        }
        if name == "horizon:clear" {
            syntax = syntax
                .arg(Arg::new("connection"))
                .arg(Arg::new("queue").long("queue"))
                .arg(Arg::new("force").long("force").action(ArgAction::SetTrue));
        }
        if name == "horizon:listen" {
            syntax = syntax
                .arg(Arg::new("environment").long("environment"))
                .arg(Arg::new("poll").long("poll").action(ArgAction::SetTrue));
        }
        Artisan::add(Box::new(Management(CommandDefinition::new(syntax), name)))?;
    }
    Ok(())
}
impl Command for Management {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            // Resource commands must work before Redis is available.
            match self.1 {
                "horizon:publish" => {
                    output.comment("Horizon no longer publishes its assets. You may stop calling the `horizon:publish` command.");
                    return Ok(0);
                }
                "horizon:install" => {
                    return super::install::install(app, output).await;
                }
                "horizon:listen" => {
                    return super::listen::listen(app, input, output).await;
                }
                "horizon:timeout" => {
                    let config = app.config().map_err(error)?;
                    let env = input
                        .matches()
                        .get_one::<String>("environment")
                        .expect("default");
                    let timeout =
                        SupervisorOptions::from_config_for_environment(config.as_ref(), env)
                            .map_err(error)?
                            .values()
                            .map(|options| options.timeout)
                            .max()
                            .unwrap_or(60);
                    output.info(&format!(
                        "Maximum timeout for {env} environment: {timeout} seconds."
                    ));
                    return Ok(0);
                }
                _ => {}
            }
            let repository = app.make::<Repository>().await.map_err(error)?;
            match self.1 {
                "horizon:list" | "horizon:supervisors" => {
                    let values = if self.1 == "horizon:list" {
                        repository.masters().await
                    } else {
                        repository.supervisors().await
                    }
                    .map_err(error)?;
                    if values.is_empty() {
                        output.info(if self.1 == "horizon:list" {
                            "No machines are running."
                        } else {
                            "No supervisors are running."
                        });
                    } else {
                        output.stdout.push_str(if self.1 == "horizon:list" {
                            "Name\tPID\tSupervisors\tStatus\n"
                        } else {
                            "Name\tPID\tStatus\tWorkers\tBalancing\n"
                        });
                        for value in values {
                            if self.1 == "horizon:list" {
                                output.stdout.push_str(&format!(
                                    "{}\t{}\t{}\t{}\n",
                                    value["name"].as_str().unwrap_or_default(),
                                    value["pid"],
                                    value["supervisors"]
                                        .as_array()
                                        .map(|names| {
                                            let names = names
                                                .iter()
                                                .filter_map(|name| name.as_str())
                                                .map(|name| {
                                                    name.split_once(':')
                                                        .map_or(name, |(_, suffix)| suffix)
                                                })
                                                .collect::<Vec<_>>();
                                            if names.is_empty() {
                                                "None".into()
                                            } else {
                                                names.join(", ")
                                            }
                                        })
                                        .unwrap_or_else(|| "None".into()),
                                    value["status"].as_str().unwrap_or_default()
                                ));
                            } else {
                                output.stdout.push_str(&format!(
                                    "{}\t{}\t{}\t{}\t{}\n",
                                    value["name"].as_str().unwrap_or_default(),
                                    value["pid"],
                                    value["status"].as_str().unwrap_or_default(),
                                    value["processes"]
                                        .as_object()
                                        .map(|processes| processes
                                            .iter()
                                            .map(|(queue, count)| format!("{queue} ({count})"))
                                            .collect::<Vec<_>>()
                                            .join(", "))
                                        .unwrap_or_default(),
                                    value["options"]["balance"]
                                        .as_str()
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| value["options"]["balance"].to_string())
                                ));
                            }
                        }
                    }
                }
                "horizon:supervisor-status"
                | "horizon:pause-supervisor"
                | "horizon:continue-supervisor" => {
                    let suffix = input.matches().get_one::<String>("name").expect("required");
                    let selected = repository
                        .supervisors()
                        .await
                        .map_err(error)?
                        .into_iter()
                        .find(|value| {
                            value["name"].as_str().is_some_and(|name| {
                                name == suffix || name.ends_with(&format!(":{suffix}"))
                            })
                        });
                    let Some(selected) = selected else {
                        output.error(if self.1 == "horizon:supervisor-status" {
                            "Unable to find a supervisor with this name."
                        } else {
                            "Failed to find a supervisor with this name"
                        });
                        return Ok(1);
                    };
                    let name = selected["name"]
                        .as_str()
                        .ok_or_else(|| error(crate::Error::InvalidRecord))?;
                    if self.1 == "horizon:supervisor-status" {
                        output.info(&format!(
                            "{suffix} is {}",
                            selected["status"].as_str().unwrap_or_default()
                        ));
                    } else if self.1 == "horizon:pause-supervisor" {
                        repository.pause_supervisor(name).await.map_err(error)?;
                        signal_supervisor(&selected, "USR2").await?;
                        output.info(&format!(
                            "Sending USR2 signal to process: {}",
                            selected["pid"]
                        ));
                    } else {
                        repository.continue_supervisor(name).await.map_err(error)?;
                        signal_supervisor(&selected, "CONT").await?;
                        output.info(&format!(
                            "Sending CONT signal to process: {}",
                            selected["pid"]
                        ));
                    }
                }
                "horizon:snapshot" => {
                    if repository
                        .metrics_repository()
                        .acquire_snapshot_lock()
                        .await
                        .map_err(error)?
                    {
                        repository
                            .metrics_repository()
                            .snapshot()
                            .await
                            .map_err(error)?;
                        output.info("Metrics snapshot stored successfully.");
                    }
                }
                "horizon:clear-metrics" => {
                    repository
                        .metrics_repository()
                        .clear()
                        .await
                        .map_err(error)?;
                    output.info("Metrics cleared successfully.");
                }
                "horizon:clear" => {
                    let args = input.matches();
                    let config = app.config().map_err(error)?;
                    if !openrust::console::confirm_to_proceed(
                        config.get("app.env").as_ref().and_then(|v| v.as_str())
                            == Some("production"),
                        args,
                    )? {
                        return Ok(1);
                    }
                    let connection = args
                        .get_one::<String>("connection")
                        .cloned()
                        .or_else(|| {
                            config.get("horizon.defaults").and_then(|v| {
                                v.as_object()?.values().next()?["connection"]
                                    .as_str()
                                    .map(str::to_owned)
                            })
                        })
                        .unwrap_or("redis".into());
                    let queue = args
                        .get_one::<String>("queue")
                        .cloned()
                        .or_else(|| {
                            config
                                .get(&format!("queue.connections.{connection}.queue"))
                                .and_then(|v| v.as_str().map(str::to_owned))
                        })
                        .unwrap_or("default".into());
                    let manager = app
                        .make::<openrust::queue::QueueManager>()
                        .await
                        .map_err(error)?;
                    let transport = manager.connection(Some(&connection)).map_err(error)?;
                    if manager.clear_prohibited() {
                        return Err(ConsoleError::new("Queue clearing is prohibited."));
                    }
                    let clearable = transport.clearable().ok_or_else(|| {
                        ConsoleError::new("Clearing queues is not supported on this connection.")
                    })?;
                    repository.jobs().purge(&queue).await.map_err(error)?;
                    let count = clearable.clear(&queue).await.map_err(error)?;
                    output.info(&format!("Cleared {count} jobs from the [{queue}] queue."));
                }
                "horizon:purge" => {
                    super::purge::purge(&repository, input, output).await?;
                }
                _ => unreachable!("registered management command"),
            }
            Ok(0)
        })
    }
}
pub(super) async fn forget(
    app: &Application,
    input: &Input,
    output: &mut Output,
    repository: &Repository,
) -> Result<i32, ConsoleError> {
    let failer = app
        .container()
        .get::<dyn openrust::queue::failed::FailedJobProviderInterface>("queue.failer")
        .map_err(error)?;
    if input.matches().get_flag("all") {
        let mut total = 0;
        loop {
            let jobs = repository.jobs().get_failed(None).await.map_err(error)?;
            if jobs.is_empty() {
                break;
            }
            for job in jobs {
                forget_job(repository, failer.as_ref(), &job).await?;
                total += 1;
            }
        }
        output.info(&if total == 0 {
            "No failed jobs detected.".into()
        } else {
            format!("{total} failed jobs deleted successfully!")
        });
        return Ok(0);
    }
    let Some(id) = input.matches().get_one::<String>("id") else {
        output.error("No failed job ID provided.");
        return Ok(1);
    };
    let Some(job) = repository.jobs().find_failed(id).await.map_err(error)? else {
        output.error("No failed job matches the given ID.");
        return Ok(1);
    };
    forget_job(repository, failer.as_ref(), &job).await?;
    output.info("Failed job deleted successfully!");
    Ok(0)
}
async fn forget_job(
    repository: &Repository,
    failer: &dyn openrust::queue::failed::FailedJobProviderInterface,
    job: &crate::JobDetails,
) -> Result<(), ConsoleError> {
    let uuid = job.payload_uuid().map_err(error)?;
    let current = repository
        .jobs()
        .find_failed_by_uuid(uuid)
        .await
        .map_err(error)?
        .is_some_and(|current| current.record.id == job.record.id);
    if current && let Some(native) = failer.find(uuid).await.map_err(error)? {
        // The native row may have been replaced after the Redis identity check.
        // Retried deliveries keep their UUID, but carry a distinct delivery ID.
        if native.connection == job.record.connection
            && native.queue == job.record.queue
            && serde_json::to_value(&native.payload).map_err(error)? == job.payload
        {
            // Database providers compare this complete snapshot at deletion,
            // preserving a newer failure that arrives during the await.
            failer.forget_retry(&native).await.map_err(error)?;
        }
    }
    repository
        .jobs()
        .delete_failed(&job.record.id)
        .await
        .map_err(error)?;
    Ok(())
}

async fn signal_supervisor(selected: &serde_json::Value, signal: &str) -> Result<(), ConsoleError> {
    #[cfg(unix)]
    {
        let pid = selected["pid"]
            .as_u64()
            .filter(|pid| *pid > 1)
            .ok_or_else(|| error(crate::Error::InvalidRecord))?;
        openrust::process::Factory::new()
            .command([
                "kill".to_owned(),
                "-s".into(),
                signal.into(),
                pid.to_string(),
            ])
            .run()
            .await
            .map_err(error)?
            .throw()
            .map_err(error)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (selected, signal);
        return Err(ConsoleError::new("Horizon process signals require Unix."));
    }
    Ok(())
}
