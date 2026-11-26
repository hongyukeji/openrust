use std::{
    ffi::OsString,
    io::{self, Read as _},
    path::Path,
    process::Stdio,
    time::{Duration, SystemTime},
};

use clap::{Arg, ArgAction, value_parser};
use tokio::{
    net::TcpListener,
    process::{Child, ChildStdin, Command as Process},
};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{
        Application, RuntimeResult, bootstrap::LoadEnvironmentVariables, http::Kernel,
        wait_for_shutdown,
    },
};

use super::config_cache_command::ExecutableEntry;

pub(in crate::foundation) const CHILD_COMMAND: &str = "__openrust_serve_http";

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ServeCommand(CommandDefinition::new(
        clap::Command::new("serve")
            .about("Start a local development HTTP server")
            .arg(Arg::new("host").long("host").help("The host to listen on"))
            .arg(
                Arg::new("port")
                    .long("port")
                    .value_parser(value_parser!(u16))
                    .help("The port to listen on"),
            )
            .arg(
                Arg::new("tries")
                    .long("tries")
                    .value_parser(value_parser!(u16))
                    .default_value("10")
                    .help("The number of additional ports to try"),
            )
            .arg(
                Arg::new("no-reload")
                    .long("no-reload")
                    .action(ArgAction::SetTrue)
                    .help("Keep the server running when the environment file changes"),
            ),
    ))))
}

struct ServeCommand(CommandDefinition);

impl Command for ServeCommand {
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
            let matches = input.matches();
            let selected = matches
                .get_many::<String>("env")
                .and_then(|mut values| values.next().map(String::as_str));
            let environment =
                LoadEnvironmentVariables::bootstrap(app.base_path(), selected).map_err(error)?;
            let host = match matches.get_one::<String>("host") {
                Some(host) => host.as_str(),
                None => environment
                    .get_utf8("SERVER_HOST")
                    .map_err(error)?
                    .unwrap_or("127.0.0.1"),
            };
            let (host, embedded_port) = split_host(host).map_err(error)?;
            let explicit = match matches.get_one::<u16>("port") {
                Some(&port) => Some(port),
                None => environment
                    .get_utf8("SERVER_PORT")
                    .map_err(error)?
                    .map(str::parse)
                    .transpose()
                    .map_err(error)?,
            };
            let endpoint = Endpoint {
                host,
                port: explicit
                    .or(embedded_port)
                    .filter(|port| *port != 0)
                    .unwrap_or(8000),
                retries: explicit.map_or_else(
                    || *matches.get_one::<u16>("tries").expect("default retries"),
                    |_| 0,
                ),
            };
            let silent = matches.get_flag("silent");
            let announce = output.streams_to_terminal() && !matches.get_flag("quiet") && !silent;
            if !matches.get_flag("no-reload")
                && let Some(modified) = modified(&environment.file).await
            {
                if !app.container().bound::<ExecutableEntry>() {
                    return Err(ConsoleError::new(
                        "Environment reload requires the application executable entry point. Use --no-reload to run in-process.",
                    ));
                }
                return supervise(
                    app,
                    endpoint,
                    selected,
                    &environment.file,
                    modified,
                    announce,
                    silent,
                )
                .await
                .map_err(error);
            }
            let kernel = Kernel::new(app.share());
            run(kernel, &endpoint, None, announce, wait_for_shutdown())
                .await
                .map_err(error)?;
            Ok(0)
        })
    }
}

struct Endpoint<'a> {
    host: &'a str,
    port: u16,
    retries: u16,
}

impl Endpoint<'_> {
    async fn bind(&self) -> io::Result<TcpListener> {
        let last = self.port.saturating_add(self.retries);
        for port in self.port..=last {
            match TcpListener::bind((self.host, port)).await {
                Ok(listener) => return Ok(listener),
                Err(error) if error.kind() == io::ErrorKind::AddrInUse && port < last => {}
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("cannot bind HTTP listener on {}:{port}: {error}", self.host),
                    ));
                }
            }
        }
        unreachable!("at least one port")
    }
}

fn split_host(address: &str) -> RuntimeResult<(&str, Option<u16>)> {
    if let Some(bracketed) = address.strip_prefix('[') {
        let (host, suffix) = bracketed
            .split_once(']')
            .ok_or_else(|| io::Error::other("invalid bracketed host"))?;
        let port = if suffix.is_empty() {
            None
        } else {
            Some(
                suffix
                    .strip_prefix(':')
                    .ok_or_else(|| io::Error::other("invalid host port"))?
                    .parse()?,
            )
        };
        return Ok((host, port));
    }
    if address.bytes().filter(|&byte| byte == b':').count() == 1 {
        let (host, port) = address.split_once(':').expect("one separator");
        return Ok((host, Some(port.parse()?)));
    }
    Ok((address, None))
}

async fn run(
    mut kernel: Kernel,
    endpoint: &Endpoint<'_>,
    ready: Option<&Path>,
    announce: bool,
    shutdown: impl Future<Output = io::Result<()>>,
) -> RuntimeResult<()> {
    kernel.bootstrap().await?;
    let listener = endpoint.bind().await?;
    let address = listener.local_addr()?;
    if let Some(ready) = ready {
        let pending = ready.with_extension("tmp");
        tokio::fs::write(&pending, address.port().to_string()).await?;
        tokio::fs::rename(pending, ready).await?;
    }
    if announce {
        println!("Server running on [http://{address}]. Press Ctrl+C to stop the server.");
    }
    kernel.serve_development(listener, shutdown).await
}

async fn supervise(
    app: &Application,
    mut endpoint: Endpoint<'_>,
    selected: Option<&str>,
    environment_file: &Path,
    mut last_modified: SystemTime,
    announce: bool,
    silent: bool,
) -> RuntimeResult<i32> {
    let executable = std::env::current_exe()?;
    let staging = tempfile::tempdir()?;
    let ready = staging.path().join("address");
    let signal = wait_for_shutdown();
    tokio::pin!(signal);
    loop {
        let mut command = Process::new(&executable);
        command
            .args(["artisan", CHILD_COMMAND, endpoint.host])
            .arg(endpoint.port.to_string())
            .arg(endpoint.retries.to_string())
            .arg(if announce { "1" } else { "0" })
            .arg(&ready)
            .args(selected)
            .env("APP_BASE_PATH", app.base_path())
            .stdin(Stdio::piped())
            .kill_on_drop(true);
        if silent {
            command.stderr(Stdio::null());
        }
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        let mut child = command.spawn()?;
        let control = child.stdin.take().expect("piped shutdown channel");
        let mut poll = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                biased;
                stop_signal = &mut signal => {
                    stop(&mut child, control).await?;
                    stop_signal?;
                    return Ok(0);
                }
                status = child.wait() => return Ok(status?.code().unwrap_or(1)),
                _ = poll.tick() => {
                    if let Ok(port) = tokio::fs::read_to_string(&ready).await
                        && let Ok(port) = port.parse()
                    {
                        endpoint.port = port;
                        endpoint.retries = 0;
                    }
                    if let Some(current) = modified(environment_file).await
                        && current != last_modified
                    {
                        last_modified = current;
                        stop(&mut child, control).await?;
                        if ready.exists() {
                            tokio::fs::remove_file(&ready).await?;
                        }
                        if announce {
                            println!("Environment modified. Restarting server...");
                        }
                        break;
                    }
                }
            }
        }
    }
}

async fn modified(path: &Path) -> Option<SystemTime> {
    tokio::fs::metadata(path).await.ok()?.modified().ok()
}

async fn stop(child: &mut Child, control: ChildStdin) -> io::Result<()> {
    drop(control);
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(result) => result.map(|_| ()),
        Err(_) => child.kill().await,
    }
}

pub(in crate::foundation) async fn child(
    mut app: Application,
    arguments: &[OsString],
) -> RuntimeResult<()> {
    let [host, port, tries, announce, ready, environment @ ..] = arguments else {
        return Err(io::Error::other("missing development server arguments").into());
    };
    let text = |value: &OsString| {
        value
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("invalid development server argument"))
    };
    let host = text(host)?;
    let endpoint = Endpoint {
        host: &host,
        port: text(port)?.parse()?,
        retries: text(tries)?.parse()?,
    };
    let selected = environment.first().map(text).transpose()?;
    app.boot_with_environment(selected.as_deref()).await?;
    run(
        Kernel::new(app),
        &endpoint,
        Some(Path::new(ready)),
        announce == "1",
        async {
            // Closing the supervisor's stdin pipe drains the current HTTP server.
            tokio::task::spawn_blocking(|| io::stdin().read(&mut [0])).await??;
            Ok(())
        },
    )
    .await
}

fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
