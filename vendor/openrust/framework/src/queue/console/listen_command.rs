use std::{ffi::OsString, io::Write as _, time::Duration};

use clap::{Arg, Command as Syntax};
use tokio_util::sync::CancellationToken;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    process::{Command as ProcessCommand, Factory, OutputStream, PendingProcess},
    queue::{QueueManager, worker::current_memory},
};

use super::work_command::{error, flag, memory_limit, seconds};

struct ListenCommand(CommandDefinition);

impl Command for ListenCommand {
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
            let container = app.container();
            let manager = container.make::<QueueManager>().map_err(error)?;
            let connection = args.get_one::<String>("connection").map(String::as_str);
            let driver = manager.connection(connection).map_err(error)?;
            let queue = args
                .get_one::<String>("queue")
                .map(String::as_str)
                .filter(|queue| !queue.is_empty())
                .unwrap_or_else(|| driver.default_queue());
            let memory = args.get_one::<String>("memory").expect("Clap default");
            let budget = app
                .config()
                .map_err(error)?
                .get("queue.memory_limit")
                .and_then(|value| value.as_u64());
            let memory_bytes = memory_limit(memory, budget)?;
            let number = |name| *args.get_one::<u32>(name).expect("Clap default");
            let timeout = Duration::from_secs(number("timeout").into());
            let rest = Duration::from_secs(number("rest").into());
            let quiet = args.get_flag("quiet") || args.get_flag("silent");
            let worker_name = args.get_one::<String>("name").expect("Clap default");
            let backoff = args
                .get_one::<u32>("backoff")
                .copied()
                .unwrap_or_else(|| number("delay"));
            let mut command: Vec<OsString> = vec![
                std::env::current_exe().map_err(error)?.into_os_string(),
                "artisan".into(),
                "queue:work".into(),
            ];
            command.extend(connection.map(OsString::from));
            command.extend([
                "--once".into(),
                "--listen-once".into(),
                "--timeout=0".into(),
                format!("--name={worker_name}").into(),
                format!("--queue={queue}").into(),
                format!("--backoff={backoff}").into(),
                format!("--memory={memory}").into(),
                format!("--sleep={}", number("sleep")).into(),
                format!("--tries={}", number("tries")).into(),
            ]);
            for option in ["force", "quiet", "silent", "ansi", "no-ansi"] {
                if args.get_flag(option) {
                    command.push(format!("--{option}").into());
                }
            }
            if let Some(environment) = args.get_many::<String>("env").and_then(Iterator::last) {
                command.push(format!("--env={environment}").into());
            }
            let command = ProcessCommand::arguments(command);
            let factory = container.make::<Factory>().map_err(error)?;
            let root = app.base_path();
            let shutdown = CancellationToken::new();
            let monitor = crate::console::monitor_signals(shutdown.clone()).map_err(error)?;
            if !quiet {
                let noun = if queue.contains(',') {
                    "queues"
                } else {
                    "queue"
                };
                output.info(&format!("Processing jobs from the [{queue}] {noun}."));
                if output.streams_to_terminal() {
                    forward(
                        OutputStream::Stdout,
                        std::mem::take(&mut output.stdout).as_bytes(),
                    );
                }
            }
            let result = listen(
                || {
                    factory
                        .command(command.clone())
                        .path(root)
                        .env([("APP_BASE_PATH", root.as_os_str())])
                        .timeout(timeout)
                },
                output,
                &shutdown,
                rest,
                memory_bytes,
            )
            .await;
            monitor.abort();
            result
        })
    }
}

async fn listen(
    process: impl Fn() -> PendingProcess,
    output: &mut Output,
    shutdown: &CancellationToken,
    rest: Duration,
    memory: u64,
) -> Result<i32, ConsoleError> {
    while !shutdown.is_cancelled() {
        let mut worker = if output.streams_to_terminal() {
            process().startWithOutput(forward)
        } else {
            process().start()
        }
        .map_err(error)?;
        let result = tokio::select! {
            result = worker.wait_ref() => result,
            _ = shutdown.cancelled() => worker.stop(Duration::from_secs(5)).await,
        };
        let result = result.map_err(error)?;
        if !output.streams_to_terminal() {
            output
                .stdout
                .push_str(&String::from_utf8_lossy(result.output()));
            output
                .stderr
                .push_str(&String::from_utf8_lossy(result.errorOutput()));
        }
        if memory > 0 && current_memory().is_some_and(|resident| resident >= memory) {
            return Ok(0);
        }
        if !rest.is_zero() {
            tokio::select! {
                _ = shutdown.cancelled() => {},
                _ = tokio::time::sleep(rest) => {},
            }
        }
    }
    Ok(0)
}

fn forward(stream: OutputStream, bytes: &[u8]) {
    match stream {
        OutputStream::Stdout => {
            let mut output = std::io::stdout().lock();
            let _ = output.write_all(bytes).and_then(|()| output.flush());
        }
        OutputStream::Stderr => {
            let mut output = std::io::stderr().lock();
            let _ = output.write_all(bytes).and_then(|()| output.flush());
        }
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:listen")
        .about("Listen to a given queue")
        .arg(Arg::new("connection"))
        .arg(Arg::new("name").long("name").default_value("default"))
        .arg(
            seconds("delay")
                .default_value("0")
                .help("Deprecated alias for --backoff"),
        )
        .arg(seconds("backoff"))
        .arg(flag("force"))
        .arg(
            Arg::new("memory")
                .long("memory")
                .default_value("128")
                .allow_hyphen_values(true),
        )
        .arg(Arg::new("queue").long("queue"))
        .arg(seconds("sleep").default_value("3"))
        .arg(seconds("rest").default_value("0"))
        .arg(seconds("timeout").default_value("60"))
        .arg(seconds("tries").default_value("1"));
    artisan.add_command(Box::new(ListenCommand(CommandDefinition::new(syntax))))
}
