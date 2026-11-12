mod restart_server;
mod start_server;

use openrust::{
    console::{CommandDefinition, ConsoleError},
    support::facades::Artisan,
};

use restart_server::RestartServer;
use start_server::StartServer;

const RESTART: &str = "openrust:reverb:restart";

pub(crate) fn register() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(StartServer(CommandDefinition::new(
        clap::Command::new("reverb:start")
            .about("Start the Reverb server")
            .arg(clap::Arg::new("host").long("host"))
            .arg(
                clap::Arg::new("port")
                    .long("port")
                    .value_parser(clap::value_parser!(u16)),
            )
            .arg(clap::Arg::new("path").long("path")),
    ))))?;
    Artisan::add(Box::new(RestartServer(CommandDefinition::new(
        clap::Command::new("reverb:restart").about("Restart the Reverb server"),
    ))))
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
