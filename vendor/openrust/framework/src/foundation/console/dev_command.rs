use std::{
    ffi::OsString,
    io::{self, Write as _},
    path::Path,
    process::{ExitStatus, Stdio},
    time::{Duration, Instant, SystemTime},
};

use clap::{Arg, ArgAction, value_parser};
use futures_util::{StreamExt as _, stream::FuturesUnordered};
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, BufReader},
    process::Command as NativeCommand,
    sync::{mpsc, oneshot, watch},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::{
        Application, DevCommandMode, DevCommands, DevProcess, dev_commands::RegisteredProcess,
    },
    process::{ChildGroup, Command as ProcessCommand, Signal},
};

use super::dev_terminal::{Action, Terminal};

const RESTART_DELAY: Duration = Duration::from_secs(1);
const MAX_RESTARTS: u8 = 5;
// Artisan children may already be draining their own workers or HTTP children.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(6);

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let mut syntax = clap::Command::new("dev").about("Run the dev processes");
    for (name, help) in [
        ("stream", "Show interleaved output in the terminal"),
        ("tabs", "Show one tab for each process"),
        ("inline", "Print labelled process output inline"),
        ("timestamps", "Display timestamps on each output line"),
        ("no-restart", "Disable auto-restart on crash"),
        ("json", "Emit newline-delimited JSON events"),
    ] {
        let mut option = Arg::new(name)
            .long(name)
            .help(help)
            .action(ArgAction::SetTrue);
        option = match name {
            "stream" => option.short('s'),
            "tabs" => option.short('t'),
            "inline" => option.short('i'),
            _ => option,
        };
        syntax = syntax.arg(option);
    }
    for name in ["buffer-size", "stream-buffer-size"] {
        syntax = syntax.arg(Arg::new(name).long(name).value_parser(value_parser!(usize)));
    }
    artisan.add_command(Box::new(DevCommand(CommandDefinition::new(syntax))))
}

struct DevCommand(CommandDefinition);

impl Command for DevCommand {
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
            let processes = DevCommands::registered()?;
            if processes.is_empty() {
                return Err(ConsoleError::new(
                    "Your application doesn't have any dev processes.",
                ));
            }
            let args = input.matches();
            let selected = args.get_many::<String>("env").and_then(Iterator::last);
            let restart = !args.get_flag("no-restart") && DevCommands::shouldAutoRestart()?;
            let mode = if args.get_flag("json") || args.get_flag("silent") || args.get_flag("quiet")
            {
                DevCommandMode::Inline
            } else if args.get_flag("tabs") {
                DevCommandMode::Tabs
            } else if args.get_flag("stream") {
                DevCommandMode::Stream
            } else if args.get_flag("inline") {
                DevCommandMode::Inline
            } else {
                DevCommands::mode()?
            };
            let timestamps = args.get_flag("timestamps") || DevCommands::shouldIncludeTimestamps()?;
            let mut terminal = if output.streams_to_terminal() && mode != DevCommandMode::Inline {
                let limit = args
                    .get_one::<usize>("buffer-size")
                    .copied()
                    .or(DevCommands::getBufferSize()?)
                    .unwrap_or(2000);
                let stream_limit = args
                    .get_one::<usize>("stream-buffer-size")
                    .copied()
                    .or(DevCommands::getStreamBufferSize()?)
                    .unwrap_or(10000);
                Terminal::open(
                    mode,
                    &processes,
                    limit,
                    stream_limit,
                    timestamps,
                    output.is_decorated(),
                )
            } else {
                None
            };
            let interactive = terminal.is_some();
            let terminate = CancellationToken::new();
            let (send, mut receive) = mpsc::channel(64);
            let mut runs = FuturesUnordered::new();
            let mut restart_controls = Vec::new();
            for (index, process) in processes.iter().enumerate() {
                let changes = interactive.then(|| {
                    let (control, changes) = watch::channel(0u64);
                    restart_controls.push(control);
                    changes
                });
                let invocation = Invocation {
                    process,
                    root: app.base_path(),
                    environment: selected.map(String::as_str),
                    restart,
                };
                runs.push(manage(
                    invocation,
                    &terminate,
                    Reporter { index, send: &send },
                    changes,
                ));
            }

            let mut report = OutputMode {
                json: args.get_flag("json"),
                timestamps,
                silent: args.get_flag("silent"),
                quiet: args.get_flag("quiet"),
                output,
            };
            let stopping = shutdown(input, report.output.streams_to_terminal());
            tokio::pin!(stopping);
            let mut refresh = tokio::time::interval(Duration::from_millis(50));
            refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut status = 0;
            while !runs.is_empty() {
                tokio::select! {
                    result = &mut stopping, if !terminate.is_cancelled() => {
                        terminate.cancel();
                        if let Err(error) = result {
                            status = 1;
                            if let Some(view) = terminal.as_mut() {
                                view.message(error.to_string());
                            } else {
                                report.emit(None, Event::Error { message: error.to_string() });
                            }
                        }
                    }
                    action = Terminal::action(&mut terminal), if interactive && !terminate.is_cancelled() => {
                        match action {
                            Action::Quit => terminate.cancel(),
                            Action::Restart(index) => restart_controls[index].send_modify(|generation| *generation = generation.wrapping_add(1)),
                            Action::Continue => {},
                        }
                    }
                    _ = refresh.tick(), if interactive => {
                        if terminal.as_mut().is_some_and(|view| view.draw().is_err()) {
                            terminate.cancel();
                        }
                    }
                    Some(code) = runs.next() => {
                        if status == 0 {
                            status = code;
                        }
                        if cfg!(windows) && !restart && code != 0 {
                            terminate.cancel();
                        }
                    }
                    Some((index, event)) = receive.recv() => {
                        if let Some(view) = terminal.as_mut() {
                            view.record(index, event);
                        } else {
                            report.emit(Some(&processes[index].definition), event);
                        }
                    }
                }
            }
            while let Ok((index, event)) = receive.try_recv() {
                if let Some(view) = terminal.as_mut() {
                    view.record(index, event);
                } else {
                    report.emit(Some(&processes[index].definition), event);
                }
            }
            if let Some(view) = terminal {
                view.finish(|index, event, time| {
                    report.emit_at(
                        index.map(|index| &processes[index].definition),
                        event,
                        Some(time),
                    );
                });
            }
            report.emit(None, Event::Done { code: status });
            Ok(status)
        })
    }
}

async fn shutdown(input: &Input, terminal: bool) -> io::Result<()> {
    let requested = async {
        match input.shutdown() {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    if !terminal {
        requested.await;
        return Ok(());
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut hangup = signal(SignalKind::hangup())?;
        let mut quit = signal(SignalKind::quit())?;
        tokio::select! {
            result = crate::foundation::wait_for_shutdown() => result?,
            _ = hangup.recv() => {},
            _ = quit.recv() => {},
            () = requested => {},
        }
    }
    #[cfg(not(unix))]
    tokio::select! {
        result = crate::foundation::wait_for_shutdown() => result?,
        () = requested => {},
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Reporter<'a> {
    index: usize,
    send: &'a mpsc::Sender<(usize, Event)>,
}

impl Reporter<'_> {
    async fn emit(self, event: Event) {
        let _ = self.send.send((self.index, event)).await;
    }
}

#[derive(Clone, Copy)]
struct Invocation<'a> {
    process: &'a RegisteredProcess,
    root: &'a Path,
    environment: Option<&'a str>,
    restart: bool,
}

async fn manage(
    call: Invocation<'_>,
    terminate: &CancellationToken,
    reporter: Reporter<'_>,
    changes: Option<watch::Receiver<u64>>,
) -> i32 {
    let Some(mut changes) = changes else {
        return supervise(call, terminate, reporter).await;
    };
    while !terminate.is_cancelled() {
        let stop = CancellationToken::new();
        let cycle = supervise(call, &stop, reporter);
        tokio::pin!(cycle);
        let retry = tokio::select! {
            () = terminate.cancelled() => {
                stop.cancel();
                cycle.await;
                false
            }
            changed = changes.changed() => {
                stop.cancel();
                cycle.await;
                changed.is_ok()
            }
            _ = &mut cycle => {
                tokio::select! {
                    () = terminate.cancelled() => false,
                    changed = changes.changed() => changed.is_ok(),
                }
            }
        };
        if !retry {
            break;
        }
    }
    0
}

async fn supervise(
    call: Invocation<'_>,
    terminate: &CancellationToken,
    reporter: Reporter<'_>,
) -> i32 {
    for attempt in 0..=MAX_RESTARTS {
        if terminate.is_cancelled() {
            return 0;
        }
        let (code, uptime) = match run_once(
            call.process,
            call.root,
            call.environment,
            terminate,
            reporter,
        )
        .await
        {
            Ok(result) => result,
            Err(_) if terminate.is_cancelled() => return 0,
            Err(error) => {
                reporter
                    .emit(Event::Error {
                        message: error.to_string(),
                    })
                    .await;
                reporter
                    .emit(Event::Failed {
                        code: 1,
                        reason: "spawn-error",
                    })
                    .await;
                return 1;
            }
        };
        if code == 0 || terminate.is_cancelled() {
            return 0;
        }
        let reason = if !call.restart {
            Some("restart-disabled")
        } else if uptime < RESTART_DELAY {
            Some("crashed-immediately")
        } else if attempt == MAX_RESTARTS {
            Some("attempts-exhausted")
        } else {
            None
        };
        if let Some(reason) = reason {
            reporter.emit(Event::Failed { code, reason }).await;
            return code;
        }
        reporter
            .emit(Event::Restarting {
                attempt: attempt + 1,
                max: MAX_RESTARTS,
            })
            .await;
        tokio::select! {
            () = sleep(RESTART_DELAY) => {},
            () = terminate.cancelled() => return 0,
        }
    }
    unreachable!("final attempt returns")
}

async fn run_once(
    process: &RegisteredProcess,
    root: &Path,
    environment: Option<&str>,
    terminate: &CancellationToken,
    reporter: Reporter<'_>,
) -> io::Result<(i32, Duration)> {
    let mut command = build_command(process, environment)?;
    command
        .current_dir(root)
        .env("APP_BASE_PATH", root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGroup::spawn_dev(&mut command).await?;
    let started = Instant::now();
    reporter
        .emit(Event::Start {
            command: process.definition.command.clone(),
            pid: child.id(),
        })
        .await;
    let (stdout, stderr) = child.output_pipes();
    let (drained, output_closed) = oneshot::channel();
    let force = CancellationToken::new();
    let exited = async {
        wait(&mut child, terminate, output_closed, &force)
            .await
            .map(|status| (status, started.elapsed()))
    };
    let output = async {
        tokio::try_join!(
            drain(stdout, "stdout", reporter, &force),
            drain(stderr, "stderr", reporter, &force)
        )?;
        let _ = drained.send(());
        Ok(())
    };
    let ((status, uptime), ()) = tokio::try_join!(exited, output)?;
    let (code, signal) = exit_details(status);
    reporter
        .emit(Event::Exit {
            code: status.code(),
            signal,
        })
        .await;
    Ok((code, uptime))
}

fn build_command(
    process: &RegisteredProcess,
    environment: Option<&str>,
) -> io::Result<NativeCommand> {
    if !process.artisan {
        #[cfg(windows)]
        {
            let mut command = windows_child()?;
            command.arg("shell").arg(&process.definition.command);
            return Ok(command);
        }
        #[cfg(not(windows))]
        return ProcessCommand::from(process.definition.command.as_str())
            .native()
            .map_err(io::Error::other);
    }
    let arguments = process
        .definition
        .command
        .strip_prefix("./artisan ")
        .expect("Artisan process");
    let arguments =
        shlex::split(arguments).ok_or_else(|| io::Error::other("invalid Artisan arguments"))?;
    #[cfg(windows)]
    let mut command = windows_child()?;
    #[cfg(not(windows))]
    let mut command = NativeCommand::new(std::env::current_exe()?);
    command.arg("artisan").args(arguments);
    if let Some(environment) = environment {
        command.args([OsString::from("--env"), OsString::from(environment)]);
    }
    Ok(command)
}

#[cfg(windows)]
pub(in crate::foundation) const WINDOWS_CHILD: &str = "__openrust_dev_child";

#[cfg(windows)]
fn windows_child() -> io::Result<NativeCommand> {
    let mut command = NativeCommand::new(std::env::current_exe()?);
    command.args(["artisan", WINDOWS_CHILD]);
    Ok(command)
}

#[cfg(windows)]
pub(in crate::foundation) async fn run_shell_child(script: OsString) -> io::Result<i32> {
    // The shell and its descendants inherit the process group. Keep this broker
    // alive through Ctrl+Break so the supervisor can observe the actual shell exit.
    let _interrupt = tokio::signal::windows::ctrl_break()?;
    let status = ProcessCommand::Shell(script)
        .native()
        .map_err(io::Error::other)?
        .stdin(Stdio::null())
        .status()
        .await?;
    Ok(status.code().unwrap_or(1))
}

async fn wait(
    child: &mut ChildGroup,
    terminate: &CancellationToken,
    output_closed: oneshot::Receiver<()>,
    force: &CancellationToken,
) -> io::Result<ExitStatus> {
    tokio::select! {
        status = child.wait() => status,
        () = terminate.cancelled() => {
            child.signal(Signal::Terminate)?;
            let clean_exit = async {
                child.wait_until_exited().await?;
                let _ = output_closed.await;
                Ok::<_, io::Error>(())
            };
            match timeout(SHUTDOWN_GRACE, clean_exit).await {
                Ok(result) => result?,
                Err(_) => {
                    force.cancel();
                    child.signal(Signal::Kill)?;
                }
            }
            child.wait().await
        }
    }
}

async fn drain(
    stream: impl AsyncRead + Unpin,
    name: &'static str,
    reporter: Reporter<'_>,
    force: &CancellationToken,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        let mut chunk = (&mut reader).take(64 * 1024);
        let count = tokio::select! {
            count = chunk.read_until(b'\n', &mut bytes) => count?,
            () = force.cancelled() => return Ok(()),
        };
        if count == 0 {
            return Ok(());
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        reporter.emit(Event::Output { stream: name, text }).await;
    }
}

fn exit_details(status: ExitStatus) -> (i32, Option<&'static str>) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        let signal = status.signal();
        let name = signal.and_then(|signal| {
            Some(match signal {
                libc::SIGINT => "SIGINT",
                libc::SIGTERM => "SIGTERM",
                libc::SIGKILL => "SIGKILL",
                libc::SIGQUIT => "SIGQUIT",
                libc::SIGHUP => "SIGHUP",
                libc::SIGABRT => "SIGABRT",
                libc::SIGSEGV => "SIGSEGV",
                _ => return None,
            })
        });
        (
            status
                .code()
                .unwrap_or_else(|| signal.map_or(1, |signal| 128 + signal)),
            name,
        )
    }
    #[cfg(not(unix))]
    (status.code().unwrap_or(1), None)
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(super) enum Event {
    Start {
        command: String,
        pid: Option<u32>,
    },
    Output {
        stream: &'static str,
        text: String,
    },
    Exit {
        code: Option<i32>,
        signal: Option<&'static str>,
    },
    Restarting {
        attempt: u8,
        max: u8,
    },
    Error {
        message: String,
    },
    Failed {
        code: i32,
        reason: &'static str,
    },
    Done {
        code: i32,
    },
}

#[derive(Serialize)]
struct Frame<'a> {
    v: u8,
    time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
    #[serde(flatten)]
    event: Event,
}

struct OutputMode<'a> {
    json: bool,
    timestamps: bool,
    silent: bool,
    quiet: bool,
    output: &'a mut Output,
}

impl OutputMode<'_> {
    fn emit(&mut self, process: Option<&DevProcess>, event: Event) {
        self.emit_at(process, event, None);
    }

    fn emit_at(&mut self, process: Option<&DevProcess>, mut event: Event, time: Option<&str>) {
        if self.silent {
            return;
        }
        let label = process.map(|process| process.name.as_str());
        if self.json {
            if let Event::Output { text, .. } = &mut event {
                *text = console::strip_ansi_codes(text).into_owned();
            }
            let time = jiff::Timestamp::try_from(SystemTime::now())
                .expect("current time is representable")
                .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
            let frame = Frame {
                v: 1,
                time,
                label,
                event,
            };
            self.write(
                false,
                &serde_json::to_string(&frame).expect("serializable dev event"),
            );
            return;
        }

        let (stderr, text) = match event {
            Event::Output { stream, text } => (stream == "stderr", text),
            Event::Error { message } => (true, message),
            Event::Restarting { attempt, max } => (true, format!("Restarting ({attempt}/{max})")),
            Event::Failed { code, reason } => {
                (true, format!("Process exited with code {code} ({reason})"))
            }
            _ => return,
        };
        let generated;
        let time = match time {
            Some(saved) => saved,
            None if self.timestamps => {
                generated = jiff::Timestamp::try_from(SystemTime::now())
                    .expect("current time is representable")
                    .strftime("%H:%M:%S ")
                    .to_string();
                &generated
            }
            None => "",
        };
        let label = if let Some(process) = process {
            let label = format!("[{}] ", process.name);
            if self.output.is_decorated() {
                match process
                    .color
                    .strip_prefix('#')
                    .and_then(|color| u32::from_str_radix(color, 16).ok())
                {
                    Some(rgb) => format!(
                        "\x1b[38;2;{};{};{}m{label}\x1b[39m",
                        (rgb >> 16) & 255,
                        (rgb >> 8) & 255,
                        rgb & 255
                    ),
                    None => label,
                }
            } else {
                label
            }
        } else {
            String::new()
        };
        self.write(stderr, &format!("{time}{label}{text}"));
    }

    fn write(&mut self, stderr: bool, text: &str) {
        if self.quiet && !stderr {
            return;
        }
        if self.output.streams_to_terminal() {
            if stderr {
                let mut stream = io::stderr().lock();
                let _ = writeln!(stream, "{text}").and_then(|()| stream.flush());
            } else {
                let mut stream = io::stdout().lock();
                let _ = writeln!(stream, "{text}").and_then(|()| stream.flush());
            }
        } else {
            use std::fmt::Write as _;
            let target = if stderr {
                &mut self.output.stderr
            } else {
                &mut self.output.stdout
            };
            let _ = writeln!(target, "{text}");
        }
    }
}
