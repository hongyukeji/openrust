use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
};

tokio::task_local! {
    static SUPERVISOR: Option<String>;
}

pub(crate) fn isWorker() -> bool {
    SUPERVISOR.try_with(|_| ()).is_ok()
}

pub(crate) fn supervisorName() -> Option<String> {
    SUPERVISOR.try_with(Clone::clone).ok().flatten()
}

/// Horizon's worker entry point delegates execution to the framework queue worker.
pub struct WorkCommand {
    definition: CommandDefinition,
    worker: openrust::queue::console::WorkCommand,
}

impl Default for WorkCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkCommand {
    pub fn new() -> Self {
        let flag = |name: &'static str| Arg::new(name).long(name).action(ArgAction::SetTrue);
        let seconds = |name: &'static str, default: &'static str| {
            Arg::new(name)
                .long(name)
                .value_parser(value_parser!(u32))
                .default_value(default)
        };
        Self {
            definition: CommandDefinition::new(
                Syntax::new("horizon:work")
                    .hide(true)
                    .about("Start a Horizon queue worker")
                    .arg(Arg::new("connection"))
                    .arg(Arg::new("name").long("name").default_value("default"))
                    .arg(seconds("delay", "0").help("Deprecated: use --backoff"))
                    .arg(seconds("backoff", "0").value_delimiter(',').num_args(1))
                    .arg(
                        Arg::new("max-jobs")
                            .long("max-jobs")
                            .value_parser(value_parser!(u64))
                            .default_value("0"),
                    )
                    .arg(seconds("max-time", "0"))
                    .arg(flag("daemon").help("Deprecated: workers already run in daemon mode"))
                    .arg(flag("force"))
                    .arg(
                        Arg::new("memory")
                            .long("memory")
                            .default_value("128")
                            .allow_hyphen_values(true),
                    )
                    .arg(flag("once"))
                    .arg(flag("listen-once").hide(true).requires("once"))
                    .arg(flag("stop-when-empty"))
                    .arg(seconds("stop-when-empty-for", "0"))
                    .arg(Arg::new("queue").long("queue"))
                    .arg(seconds("sleep", "3"))
                    .arg(seconds("rest", "0"))
                    .arg(Arg::new("supervisor").long("supervisor"))
                    .arg(seconds("timeout", "60"))
                    .arg(seconds("tries", "0"))
                    .arg(flag("json")),
            ),
            worker: openrust::queue::console::WorkCommand::new(),
        }
    }
}

impl Command for WorkCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let supervisor = input.matches().get_one::<String>("supervisor").cloned();
            let scopedInput = input
                .shutdown()
                .map(|shutdown| input.withShutdown(shutdown.child_token()));
            #[cfg(unix)]
            let parent = rustix::process::getppid();
            let watch = async {
                #[cfg(unix)]
                {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        if supervisor.is_some() && rustix::process::getppid() != parent {
                            break;
                        }
                    }
                }
                #[cfg(not(unix))]
                std::future::pending::<()>().await;
            };
            let work = SUPERVISOR.scope(
                supervisor.clone(),
                self.worker
                    .handle(app, scopedInput.as_ref().unwrap_or(input), output),
            );
            tokio::pin!(work);
            tokio::select! {
                result = &mut work => result,
                () = watch => {
                    if let Some(shutdown)=scopedInput.as_ref().and_then(Input::shutdown) { shutdown.cancel(); }
                    else {
                        #[cfg(unix)]
                        rustix::process::kill_process(rustix::process::getpid(),rustix::process::Signal::TERM).map_err(|_|ConsoleError::new("Unable to stop orphaned Horizon worker."))?;
                    }
                    work.await
                }
            }
        })
    }
}
