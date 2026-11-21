use super::{error, signals};
use crate::{Repository, Supervisor, SupervisorOptions};
use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::foundation::Application as _,
    foundation::Application,
};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub struct SupervisorCommand(CommandDefinition);
impl Default for SupervisorCommand {
    fn default() -> Self {
        Self::new()
    }
}
impl SupervisorCommand {
    pub fn new() -> Self {
        let mut syntax = Syntax::new("horizon:supervisor")
            .hide(true)
            .about("Start a new supervisor")
            .arg(Arg::new("name").required(true))
            .arg(Arg::new("connection").required(true));
        for (name, default) in [
            ("balance", "off"),
            ("backoff", "0"),
            ("delay", "0"),
            ("max-jobs", "0"),
            ("max-time", "0"),
            ("max-processes", "1"),
            ("min-processes", "1"),
            ("memory", "128"),
            ("nice", "0"),
            ("sleep", "3"),
            ("timeout", "60"),
            ("tries", "0"),
            ("auto-scaling-strategy", "time"),
            ("balance-cooldown", "3"),
            ("balance-max-shift", "1"),
            ("workers-name", "default"),
            ("parent-id", "0"),
            ("rest", "0"),
        ] {
            syntax = syntax.arg(
                Arg::new(name)
                    .long(name)
                    .default_value(default)
                    .allow_hyphen_values(name == "nice"),
            );
        }
        for name in ["paused", "force", "json"] {
            syntax = syntax.arg(Arg::new(name).long(name).action(ArgAction::SetTrue));
        }
        syntax = syntax.arg(Arg::new("queue").long("queue")).arg(
            Arg::new("directory")
                .long("directory")
                .hide(true)
                .value_parser(value_parser!(std::path::PathBuf)),
        );
        Self(CommandDefinition::new(syntax))
    }
}
impl Command for SupervisorCommand {
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
            let args = input.matches();
            let config = app.config().map_err(error)?;
            let name = args
                .get_one::<String>("name")
                .expect("required name")
                .clone();
            let connection = args
                .get_one::<String>("connection")
                .expect("required connection")
                .clone();
            let queue = args.get_one::<String>("queue").cloned().unwrap_or_else(|| {
                config
                    .get(&format!("queue.connections.{connection}.queue"))
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or("default".into())
            });
            let mut values = serde_json::json!({"name":name,"connection":connection,"queue":queue});
            for (option, key) in [
                ("balance", "balance"),
                ("backoff", "backoff"),
                ("workers-name", "workersName"),
                ("auto-scaling-strategy", "autoScalingStrategy"),
            ] {
                values[key] = args
                    .get_one::<String>(option)
                    .expect("default")
                    .clone()
                    .into();
            }
            for (option, key) in [
                ("max-jobs", "maxJobs"),
                ("max-time", "maxTime"),
                ("max-processes", "maxProcesses"),
                ("min-processes", "minProcesses"),
                ("memory", "memory"),
                ("nice", "nice"),
                ("sleep", "sleep"),
                ("timeout", "timeout"),
                ("tries", "maxTries"),
                ("balance-cooldown", "balanceCooldown"),
                ("balance-max-shift", "balanceMaxShift"),
                ("parent-id", "parentId"),
                ("rest", "rest"),
            ] {
                values[key] = serde_json::to_value(
                    args.get_one::<String>(option)
                        .expect("default")
                        .parse::<i64>()
                        .map_err(error)?,
                )
                .map_err(error)?;
            }
            for flag in ["force", "json"] {
                values[flag] = args.get_flag(flag).into();
            }
            if let Some(directory) = args.get_one::<std::path::PathBuf>("directory") {
                values["directory"] = directory.to_string_lossy().to_string().into();
            }
            let options = SupervisorOptions::fromArray(values).map_err(error)?;
            let repository = app.make::<Repository>().await.map_err(error)?;
            if repository.supervisor(&name).await.map_err(error)?.is_some() {
                output.error("A supervisor with this name is already running.");
                return Ok(13);
            }
            crate::signals::adjustNice(options.nice).map_err(error)?;
            let supervisor = Supervisor::new(
                repository,
                app.make::<openrust::queue::QueueManager>()
                    .await
                    .map_err(error)?,
                BTreeMap::from([(name, options)]),
                std::env::current_exe().map_err(error)?,
                app.base_path().to_owned(),
                config
                    .get("app.env")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or("production".into()),
            )
            .map_err(error)?
            .paused(args.get_flag("paused"))
            .fastTermination(
                config
                    .get("horizon.fast_termination")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false),
            );
            let shutdown = input
                .shutdown()
                .cloned()
                .unwrap_or_else(CancellationToken::new);
            let signals = signals().map_err(error)?;
            let run = supervisor.run(&shutdown);
            tokio::pin!(run);
            let result = tokio::select! { result=&mut run=>result, ()=signals=>{shutdown.cancel();run.await} };
            if matches!(result, Err(crate::Error::MemoryLimitExceeded)) {
                output.error("Supervisor memory limit exceeded.");
                return Ok(12);
            }
            result.map_err(error)?;
            Ok(0)
        })
    }
}
